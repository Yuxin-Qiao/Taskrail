//! Model Context Protocol adapter for ChatGPT and other MCP hosts.
//!
//! The adapter deliberately stays at the edge of Taskrail. The daemon remains
//! the only component that owns the Registry and executes automations; this
//! process translates MCP tool calls into the daemon's local JSON-RPC calls.

use crate::rpc;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const SERVER_NAME: &str = "taskrail";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const INSTRUCTIONS: &str = "Taskrail manages scheduled automations on this host. Call taskrail_status first. When the user asks what automations exist on the local computer, call taskrail_discover_local_automations for a fresh native-scheduler scan, then use taskrail_list_automations or taskrail_get_automation for details. Use direct argv commands only. Do not claim an automation ran unless the tool result reports its run status. ChatGPT Scheduled tasks can call these tools at their scheduled time.";

#[derive(Debug, Deserialize)]
struct Request {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

pub async fn serve_stdio(socket_path: PathBuf) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        if line.trim().is_empty() {
            line.clear();
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => handle_request(request, &socket_path).await,
            Err(error) => Some(error_response(
                Value::Null,
                -32700,
                format!("invalid JSON: {error}"),
            )),
        };
        if let Some(response) = response {
            let mut payload = serde_json::to_vec(&response)?;
            payload.push(b'\n');
            stdout.write_all(&payload).await?;
            stdout.flush().await?;
        }
        line.clear();
    }
    Ok(())
}

async fn handle_request(request: Request, socket_path: &PathBuf) -> Option<Value> {
    request.id.as_ref()?;
    let id = request.id.unwrap_or(Value::Null);
    if request.jsonrpc != "2.0" {
        return Some(error_response(id, -32600, "jsonrpc must be \"2.0\"".into()));
    }
    let result = match request.method.as_str() {
        "initialize" => Ok(initialize_result(&request.params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_descriptors() })),
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        "tools/call" => call_tool(&request.params, socket_path).await,
        "notifications/initialized" => Ok(Value::Null),
        method => Err(anyhow::anyhow!("method not found: {method}")),
    };
    Some(match result {
        Ok(result) => success_response(id, result),
        Err(error) => error_response(id, -32000, error.to_string()),
    })
}

fn initialize_result(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .filter(|version| matches!(*version, "2025-06-18" | "2025-03-26" | "2024-11-05"))
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {"listChanged": false}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION
        },
        "instructions": INSTRUCTIONS
    })
}

fn tool_descriptors() -> Vec<Value> {
    vec![
        tool(
            "taskrail_status",
            "Taskrail status",
            "Use this first to verify that the Taskrail daemon is connected and identify the local Mac or Linux host.",
            object_schema(json!({}), &[]),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_list_automations",
            "List automations",
            "Use this when the user wants to see scheduled, paused, observed, or managed automations on this host.",
            object_schema(json!({}), &[]),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_discover_local_automations",
            "Discover local automations",
            "Use this when the user asks what automation tasks already exist on this Mac or Linux host. It performs a fresh read-only scan of launchd, cron, systemd, and Homebrew services, reconciles observed records, and returns safe summaries without changing native scheduler definitions.",
            object_schema(
                json!({
                    "source": {"type":"string", "enum":["all","launchd","cron","systemd","homebrew"], "default":"all"},
                }),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_scan_native",
            "Scan native schedulers",
            "Use this when the user wants Taskrail to discover launchd, cron, systemd, or Homebrew services on this host. Scanning reconciles observed records but does not modify native scheduler definitions.",
            object_schema(
                json!({
                    "source": {"type":"string", "enum":["all","launchd","cron","systemd","homebrew"]},
                }),
                &[],
            ),
            false,
            false,
            true,
        ),
        tool(
            "taskrail_get_automation",
            "Inspect automation",
            "Use this when the user wants the definition, trigger, ownership, or next run of one automation.",
            object_schema(json!({"id":{"type":"string","minLength":1}}), &["id"]),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_create_automation",
            "Create automation",
            "Use this when the user wants to schedule a direct executable on this host. Confirm the executable, arguments, working directory, and schedule from the request; do not create shell pipelines or shell strings.",
            object_schema(
                json!({
                    "id":{"type":"string","minLength":1},
                    "name":{"type":"string","minLength":1},
                    "executable":{"type":"string","minLength":1},
                    "args":{"type":"array","items":{"type":"string"}},
                    "cwd":{"type":"string","minLength":1},
                    "trigger":{"type":"string","enum":["manual","interval","cron"],"default":"manual"},
                    "interval_seconds":{"type":"integer","minimum":1},
                    "cron":{"type":"string","minLength":1},
                    "timezone":{"type":"string","default":"local"},
                    "timeout_seconds":{"type":"integer","minimum":1,"maximum":86400},
                }),
                &["id", "executable"],
            ),
            false,
            false,
            false,
        ),
        tool(
            "taskrail_pause_automation",
            "Pause automation",
            "Use this when the user wants to pause a managed automation without deleting its definition.",
            object_schema(json!({"id":{"type":"string","minLength":1}}), &["id"]),
            false,
            false,
            true,
        ),
        tool(
            "taskrail_resume_automation",
            "Resume automation",
            "Use this when the user wants to resume a paused managed automation.",
            object_schema(json!({"id":{"type":"string","minLength":1}}), &["id"]),
            false,
            false,
            true,
        ),
        tool(
            "taskrail_run_automation",
            "Run automation now",
            "Use this when the user explicitly asks to run a managed automation immediately. The command may have side effects on the host.",
            object_schema(
                json!({
                    "id":{"type":"string","minLength":1},
                    "allow_observed":{"type":"boolean","default":false},
                }),
                &["id"],
            ),
            false,
            true,
            false,
        ),
        tool(
            "taskrail_list_runs",
            "List automation runs",
            "Use this when the user wants recent run status, timing, or failure information.",
            object_schema(
                json!({
                    "automation_id":{"type":"string","minLength":1},
                    "limit":{"type":"integer","minimum":1,"maximum":500},
                }),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_get_run_logs",
            "Get run logs",
            "Use this when the user wants stdout or stderr for a specific automation run, especially after a failure.",
            object_schema(
                json!({"run_id":{"type":"string","minLength":1}}),
                &["run_id"],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_cancel_run",
            "Cancel run",
            "Use this when the user explicitly asks to stop an active automation run.",
            object_schema(
                json!({"run_id":{"type":"string","minLength":1}}),
                &["run_id"],
            ),
            false,
            true,
            true,
        ),
        tool(
            "taskrail_list_attention",
            "List attention items",
            "Use this when the user asks what needs attention, including failed runs, drift, or paused items.",
            object_schema(
                json!({"limit":{"type":"integer","minimum":1,"maximum":500}}),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_list_events",
            "List audit events",
            "Use this when the user wants a recent activity or audit trail for Taskrail.",
            object_schema(
                json!({"limit":{"type":"integer","minimum":1,"maximum":500}}),
                &[],
            ),
            true,
            false,
            true,
        ),
    ]
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn tool(
    name: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": {"type":"object","additionalProperties":true},
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "openWorldHint": false,
            "idempotentHint": idempotent,
        }
    })
}

async fn call_tool(params: &Value, socket_path: &PathBuf) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("tools/call requires params.name")?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (rpc_method, rpc_params) = match name {
        "taskrail_status" => ("daemon.status", json!({})),
        "taskrail_list_automations" => ("automation.list", json!({})),
        "taskrail_discover_local_automations" => ("automation.scan", arguments),
        "taskrail_scan_native" => ("automation.scan", arguments),
        "taskrail_get_automation" => ("automation.inspect", arguments),
        "taskrail_create_automation" => ("automation.create", arguments),
        "taskrail_pause_automation" => ("automation.pause", arguments),
        "taskrail_resume_automation" => ("automation.resume", arguments),
        "taskrail_run_automation" => ("automation.run", arguments),
        "taskrail_list_runs" => ("runs.list", arguments),
        "taskrail_get_run_logs" => ("run.logs", arguments),
        "taskrail_cancel_run" => ("run.cancel", arguments),
        "taskrail_list_attention" => ("inbox.list", arguments),
        "taskrail_list_events" => ("events.list", arguments),
        _ => anyhow::bail!("unknown Taskrail tool: {name}"),
    };
    let value = rpc::call(socket_path, rpc_method, rpc_params).await?;
    let value = if name == "taskrail_discover_local_automations" {
        sanitize_discovered_sources(value)
    } else {
        value
    };
    let value = if name == "taskrail_status" {
        // ChatGPT can cache an older tool descriptor for a connected Tunnel.
        // Keep the stable status entry useful for that client by attaching a
        // fresh, safe native-scheduler scan to it as well.
        let discovery = rpc::call(socket_path, "automation.scan", json!({"source":"all"}))
            .await
            .map(sanitize_discovered_sources)?;
        json!({
            "daemon": value,
            "host": {
                "label": std::env::var("TASKRAIL_HOST_LABEL").ok(),
                "platform": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
            },
            "local_discovery": discovery,
        })
    } else {
        value
    };
    let summary = summarize(name, &value);
    Ok(json!({
        "content": [{"type":"text","text":summary}],
        "structuredContent": {"result": value},
        "isError": false,
    }))
}

fn summarize(name: &str, value: &Value) -> String {
    match name {
        "taskrail_status" => format!(
            "Taskrail daemon connected on {} ({}), managing {} automation(s); fresh local scan found {} source(s).",
            std::env::consts::OS,
            std::env::consts::ARCH,
            value
                .get("daemon")
                .and_then(|daemon| daemon.get("automation_count"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            value
                .get("local_discovery")
                .and_then(|discovery| discovery.get("count"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ),
        "taskrail_list_automations" => format!(
            "Taskrail returned {} automation(s).",
            value.as_array().map_or(0, Vec::len)
        ),
        "taskrail_discover_local_automations" => format!(
            "Fresh local scan discovered {} automation source(s).",
            value
                .get("automations")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        "taskrail_scan_native" => format!(
            "Scanned and reconciled {} native automation source(s).",
            value.as_array().map_or(0, Vec::len)
        ),
        "taskrail_create_automation" => {
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("automation");
            format!("Created managed automation {name}.")
        }
        "taskrail_run_automation" => {
            let run_id = value
                .get("run_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            format!("Automation run {run_id} finished with status {status}.")
        }
        _ => format!("Taskrail tool {name} completed."),
    }
}

fn sanitize_discovered_sources(value: Value) -> Value {
    let automations = value
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| {
            let command = source.get("command").and_then(|command| {
                Some(json!({
                    "executable": command.get("executable")?,
                    "args": command.get("args")?,
                    "cwd": command.get("cwd"),
                    "shell": command.get("shell").unwrap_or(&Value::Bool(false)),
                }))
            });
            json!({
                "id": source.get("source_id"),
                "name": source.get("native_id"),
                "provider": source.get("provider"),
                "kind": source.get("kind"),
                "enabled": source.get("enabled"),
                "path": source.get("path"),
                "trigger": source.get("trigger"),
                "command": command,
                "ownership": "observed",
            })
        })
        .collect::<Vec<_>>();
    let mut providers = automations
        .iter()
        .filter_map(|item| item.get("provider").and_then(Value::as_str))
        .collect::<Vec<_>>();
    providers.sort_unstable();
    providers.dedup();
    json!({
        "count": automations.len(),
        "providers": providers,
        "automations": automations,
        "native_definitions_changed": false,
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":id,
        "error":{"code":code,"message":message}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_have_unique_names_and_schemas() {
        let tools = tool_descriptors();
        let mut names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), tools.len());
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert!(
                tool["description"]
                    .as_str()
                    .unwrap()
                    .starts_with("Use this")
            );
            assert!(tool["annotations"].is_object());
        }
    }

    #[test]
    fn initialize_result_advertises_tools_and_instructions() {
        let result = initialize_result(&json!({"protocolVersion":"2025-06-18"}));
        assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(result["capabilities"]["tools"]["listChanged"], false);
        assert!(
            result["instructions"]
                .as_str()
                .unwrap()
                .contains("Scheduled")
        );
        assert!(
            result["instructions"]
                .as_str()
                .unwrap()
                .contains("discover_local_automations")
        );
    }

    #[test]
    fn discovered_sources_are_summarized_without_raw_definition_or_environment() {
        let value = sanitize_discovered_sources(json!([{
            "source_id": "launchd:example",
            "native_id": "example",
            "provider": "launchd",
            "kind": "task",
            "enabled": true,
            "path": "/Users/example/Library/LaunchAgents/example.plist",
            "trigger": {"kind":"manual"},
            "command": {
                "executable": "/bin/echo",
                "args": ["hello"],
                "cwd": null,
                "env": {"TOKEN":"secret"},
                "shell": false
            },
            "raw": "secret raw plist"
        }]));
        assert_eq!(value["count"], 1);
        assert_eq!(value["automations"][0]["name"], "example");
        assert!(value["automations"][0].get("raw").is_none());
        assert!(value["automations"][0]["command"].get("env").is_none());
    }
}
