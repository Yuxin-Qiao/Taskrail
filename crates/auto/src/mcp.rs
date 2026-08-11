use crate::{service, storage::Registry};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
pub const MCP_LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(rename = "_meta", default)]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub async fn serve_stdio(registry_path: impl AsRef<Path>) -> Result<()> {
    let registry_path = registry_path.as_ref().to_path_buf();
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut input = BufReader::new(stdin);
    let mut output = tokio::io::BufWriter::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line).await.context("read MCP stdin")? == 0 {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => handle_request(request, &registry_path).await,
            Err(error) => Some(error_response(
                Value::Null,
                -32600,
                format!("invalid MCP JSON-RPC request: {error}"),
            )),
        };
        if let Some(response) = response {
            let mut payload = serde_json::to_vec(&response)?;
            payload.push(b'\n');
            output
                .write_all(&payload)
                .await
                .context("write MCP stdout")?;
            output.flush().await.context("flush MCP stdout")?;
        }
    }
}

pub async fn handle_request(request: Request, registry_path: &Path) -> Option<Response> {
    if request.id.is_none() || request.method.starts_with("notifications/") {
        return None;
    }
    let id = request.id.clone().unwrap_or(Value::Null);
    if request.jsonrpc != "2.0" {
        return Some(error_response(id, -32600, "jsonrpc must be \"2.0\"".into()));
    }
    if let Some(error) = validate_meta(&request.meta) {
        return Some(error_response(id, -32602, error));
    }
    let result = match request.method.as_str() {
        "initialize" => initialize_result(&request.params),
        "server/discover" => Ok(discover_result()),
        "ping" => Ok(serde_json::json!({})),
        "tools/list" => Ok(tools_list_result()),
        "tools/call" => return call_tool(id, request.params, registry_path).await,
        _ => {
            return Some(error_response(
                id,
                -32601,
                format!("method not found: {}", request.method),
            ));
        }
    };
    Some(match result {
        Ok(result) => Response {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        },
        Err(error) => error_response(id, -32602, error.to_string()),
    })
}

fn initialize_result(params: &Value) -> Result<Value> {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(MCP_PROTOCOL_VERSION);
    let protocol_version = match requested {
        MCP_PROTOCOL_VERSION | MCP_LEGACY_PROTOCOL_VERSION => requested,
        _ => anyhow::bail!("unsupported protocol version: {requested}"),
    };
    Ok(serde_json::json!({
        "protocolVersion": protocol_version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "auto", "version": env!("CARGO_PKG_VERSION")},
        "instructions": "This server exposes pre-registered local automations and read-only metrics. It never exposes arbitrary shell execution."
    }))
}

fn discover_result() -> Value {
    serde_json::json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "auto", "version": env!("CARGO_PKG_VERSION")}
    })
}

fn tools_list_result() -> Value {
    serde_json::json!({
        "resultType": "complete",
        "tools": [
            {
                "name": "automation_list",
                "title": "List local automations",
                "description": "List automations registered in the local control plane.",
                "inputSchema": {"type": "object", "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "automation_inspect",
                "title": "Inspect a local automation",
                "description": "Inspect one pre-registered automation by ID or name.",
                "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"], "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "automation_policy_check",
                "title": "Check automation policy",
                "description": "Run a side-effect-free policy preflight for a registered automation.",
                "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"], "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "runs_list",
                "title": "List local runs",
                "description": "List bounded recent runs with immutable automation revision snapshots.",
                "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": 500}, "automation_id": {"type": "string"}}, "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "events_list",
                "title": "List local events",
                "description": "List bounded newest-first local audit events.",
                "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": 500}}, "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "inbox_list",
                "title": "List operator attention items",
                "description": "Read a bounded aggregation of pending approvals, drift/recovery attention, and failed runs.",
                "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": 500}}, "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "automation_run",
                "title": "Run a pre-registered automation",
                "description": "Run a named local automation through the supervisor policy. This tool cannot execute an arbitrary command and does not allow observed jobs by default.",
                "inputSchema": {"type": "object", "properties": {"automation_id": {"type": "string"}}, "required": ["automation_id"], "additionalProperties": false},
                "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "run_cancel",
                "title": "Cancel an active run",
                "description": "Cancel a run owned by this local daemon. Completed or external runs cannot be rewritten.",
                "inputSchema": {"type": "object", "properties": {"run_id": {"type": "string"}}, "required": ["run_id"], "additionalProperties": false},
                "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "run_logs",
                "title": "Read run logs",
                "description": "Read bounded stdout and stderr for a recorded local run.",
                "inputSchema": {"type": "object", "properties": {"run_id": {"type": "string"}}, "required": ["run_id"], "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "approvals_list",
                "title": "List pending approvals",
                "description": "List local approval requests and their decisions.",
                "inputSchema": {"type": "object", "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            },
            {
                "name": "metrics_list",
                "title": "List local metrics",
                "description": "List locally recorded run and provider usage metrics.",
                "inputSchema": {"type": "object", "additionalProperties": false},
                "annotations": {"readOnlyHint": true, "destructiveHint": false, "openWorldHint": false}
            }
        ],
        "ttlMs": 30000,
        "cacheScope": "private"
    })
}

async fn call_tool(id: Value, params: Value, registry_path: &Path) -> Option<Response> {
    let name = match params.get("name").and_then(Value::as_str) {
        Some(name) => name,
        None => {
            return Some(error_response(
                id,
                -32602,
                "tools/call params.name must be a string".into(),
            ));
        }
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if !arguments.is_object() {
        return Some(error_response(
            id,
            -32602,
            "tools/call params.arguments must be an object".into(),
        ));
    }
    let execution = match name {
        "automation_list" => {
            let result = Registry::open(registry_path)
                .and_then(|registry| Ok(serde_json::to_value(registry.list_automations()?)?));
            result.map(tool_success).unwrap_or_else(tool_failure)
        }
        "automation_inspect" => {
            let automation_id = match arguments.get("id").and_then(Value::as_str) {
                Some(id) => id,
                None => {
                    return Some(error_response(
                        id,
                        -32602,
                        "automation_inspect requires id".into(),
                    ));
                }
            };
            match Registry::open(registry_path)
                .and_then(|registry| registry.get_automation(automation_id))
            {
                Ok(Some(value)) => tool_success(serde_json::to_value(value).unwrap_or(Value::Null)),
                Ok(None) => tool_failure(anyhow::anyhow!("automation not found: {automation_id}")),
                Err(error) => tool_failure(error),
            }
        }
        "automation_policy_check" => {
            let automation_id = match arguments.get("id").and_then(Value::as_str) {
                Some(id) => id,
                None => {
                    return Some(error_response(
                        id,
                        -32602,
                        "automation_policy_check requires id".into(),
                    ));
                }
            };
            match Registry::open(registry_path)
                .and_then(|registry| registry.get_automation(automation_id))
            {
                Ok(Some(value)) => tool_success(
                    serde_json::to_value(crate::policy::check(&value)).unwrap_or(Value::Null),
                ),
                Ok(None) => tool_failure(anyhow::anyhow!("automation not found: {automation_id}")),
                Err(error) => tool_failure(error),
            }
        }
        "runs_list" => {
            let limit = match bounded_limit(&arguments) {
                Ok(limit) => limit,
                Err(error) => return Some(error_response(id, -32602, error)),
            };
            let automation_id = match arguments.get("automation_id") {
                None => None,
                Some(value) => match value.as_str() {
                    Some(value) if !value.trim().is_empty() => Some(value),
                    _ => {
                        return Some(error_response(
                            id,
                            -32602,
                            "runs_list automation_id must be a non-empty string".into(),
                        ));
                    }
                },
            };
            let result = Registry::open(registry_path).and_then(|registry| {
                Ok(serde_json::to_value(
                    registry.list_runs(limit, automation_id)?,
                )?)
            });
            result.map(tool_success).unwrap_or_else(tool_failure)
        }
        "events_list" => {
            let limit = match bounded_limit(&arguments) {
                Ok(limit) => limit,
                Err(error) => return Some(error_response(id, -32602, error)),
            };
            let result = Registry::open(registry_path)
                .and_then(|registry| Ok(serde_json::to_value(registry.list_events(limit)?)?));
            result.map(tool_success).unwrap_or_else(tool_failure)
        }
        "inbox_list" => {
            let limit = match bounded_limit(&arguments) {
                Ok(limit) => limit,
                Err(error) => return Some(error_response(id, -32602, error)),
            };
            let result = Registry::open(registry_path)
                .and_then(|registry| Ok(serde_json::to_value(registry.list_inbox(limit)?)?));
            result.map(tool_success).unwrap_or_else(tool_failure)
        }
        "automation_run" => {
            let automation_id = match arguments.get("automation_id").and_then(Value::as_str) {
                Some(id) => id.to_owned(),
                None => {
                    return Some(error_response(
                        id,
                        -32602,
                        "automation_run requires automation_id".into(),
                    ));
                }
            };
            match service::run_named(registry_path, &automation_id, false).await {
                Ok(value) => tool_success(serde_json::to_value(value).unwrap_or(Value::Null)),
                Err(error) => tool_failure(error),
            }
        }
        "run_cancel" => {
            let run_id = match arguments.get("run_id").and_then(Value::as_str) {
                Some(run_id) if !run_id.trim().is_empty() => run_id,
                _ => {
                    return Some(error_response(
                        id,
                        -32602,
                        "run_cancel requires run_id".into(),
                    ));
                }
            };
            match service::cancel_run(registry_path, run_id) {
                Ok(()) => tool_success(serde_json::json!({
                    "run_id": run_id,
                    "status": "cancel_requested"
                })),
                Err(error) => tool_failure(error),
            }
        }
        "run_logs" => {
            let run_id = match arguments.get("run_id").and_then(Value::as_str) {
                Some(run_id) if !run_id.trim().is_empty() => run_id,
                _ => {
                    return Some(error_response(
                        id,
                        -32602,
                        "run_logs requires run_id".into(),
                    ));
                }
            };
            let result = Registry::open(registry_path).and_then(|registry| {
                let logs = registry
                    .get_run_logs(run_id)?
                    .with_context(|| format!("run not found: {run_id}"))?;
                Ok(serde_json::to_value(logs)?)
            });
            result.map(tool_success).unwrap_or_else(tool_failure)
        }
        "approvals_list" => {
            let result = Registry::open(registry_path)
                .and_then(|registry| Ok(serde_json::to_value(registry.list_approvals()?)?));
            result.map(tool_success).unwrap_or_else(tool_failure)
        }
        "metrics_list" => {
            let result = Registry::open(registry_path)
                .and_then(|registry| Ok(serde_json::to_value(registry.list_metrics()?)?));
            result.map(tool_success).unwrap_or_else(tool_failure)
        }
        _ => return Some(error_response(id, -32602, format!("unknown tool: {name}"))),
    };
    Some(Response {
        jsonrpc: "2.0",
        id,
        result: Some(execution),
        error: None,
    })
}

fn bounded_limit(arguments: &Value) -> Result<usize, String> {
    match arguments.get("limit") {
        None => Ok(100),
        Some(value) => match value.as_u64() {
            Some(value) if (1..=500).contains(&value) => Ok(value as usize),
            _ => Err("limit must be an integer from 1 to 500".into()),
        },
    }
}

fn tool_success(value: Value) -> Value {
    serde_json::json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into())}],
        "structuredContent": value,
        "isError": false
    })
}

fn tool_failure(error: anyhow::Error) -> Value {
    serde_json::json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": error.to_string()}],
        "isError": true
    })
}

fn validate_meta(meta: &Option<Value>) -> Option<String> {
    let Some(meta) = meta else {
        return None;
    };
    let version = meta
        .get("io.modelcontextprotocol/protocolVersion")
        .or_else(|| meta.get("protocolVersion"))
        .and_then(Value::as_str);
    match version {
        Some(MCP_PROTOCOL_VERSION | MCP_LEGACY_PROTOCOL_VERSION) | None => None,
        Some(version) => Some(format!("unsupported MCP protocol version: {version}")),
    }
}

fn error_response(id: Value, code: i32, message: String) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(ErrorObject {
            code,
            message,
            data: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CommandSpec, Ownership, StepSpec};
    use tempfile::tempdir;

    fn request(id: u64, method: &str, params: Value) -> Request {
        Request {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(id)),
            method: method.into(),
            params,
            meta: Some(
                serde_json::json!({"io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION}),
            ),
        }
    }

    #[tokio::test]
    async fn discover_and_initialize_advertise_tools_without_sessions() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let discover = handle_request(request(1, "server/discover", Value::Null), &path)
            .await
            .unwrap();
        assert_eq!(
            discover.result.unwrap()["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        let initialize = handle_request(
            request(
                2,
                "initialize",
                serde_json::json!({"protocolVersion": MCP_LEGACY_PROTOCOL_VERSION}),
            ),
            &path,
        )
        .await
        .unwrap();
        assert_eq!(
            initialize.result.unwrap()["protocolVersion"],
            MCP_LEGACY_PROTOCOL_VERSION
        );
        let list = handle_request(request(3, "tools/list", Value::Null), &path)
            .await
            .unwrap();
        let tools = list.result.unwrap()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 11);
        assert!(
            tools
                .iter()
                .all(|tool| tool["name"] != "shell" && tool["name"] != "exec")
        );
    }

    #[tokio::test]
    async fn tool_call_runs_only_registered_automation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        Registry::open(&path)
            .unwrap()
            .save_automation(&crate::core::Automation {
                id: "mcp-a".into(),
                name: "mcp-a".into(),
                ownership: Ownership::Managed,
                steps: vec![StepSpec {
                    id: "echo".into(),
                    command: CommandSpec::argv("/bin/echo", ["mcp-ok"]),
                    responses: None,
                    risk: crate::core::Risk::R0Read,
                }],
                ..Default::default()
            })
            .unwrap();
        let response = handle_request(
            request(
                4,
                "tools/call",
                serde_json::json!({"name":"automation_run","arguments":{"automation_id":"mcp-a"}}),
            ),
            &path,
        )
        .await
        .unwrap();
        assert_eq!(response.result.unwrap()["isError"], false);
        let unknown = handle_request(
            request(
                5,
                "tools/call",
                serde_json::json!({"name":"shell","arguments":{"command":"echo nope"}}),
            ),
            &path,
        )
        .await
        .unwrap();
        assert_eq!(unknown.error.unwrap().code, -32602);
    }

    #[tokio::test]
    async fn tool_business_failures_are_is_error_results() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let response = handle_request(request(6, "tools/call", serde_json::json!({"name":"automation_run","arguments":{"automation_id":"missing"}})), &path).await.unwrap();
        assert_eq!(response.result.unwrap()["isError"], true);
    }
}
