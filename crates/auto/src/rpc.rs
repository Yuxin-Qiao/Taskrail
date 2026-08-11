use crate::{
    core::{ApprovalState, RuntimeState},
    service,
    storage::Registry,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
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

pub async fn serve(socket_path: impl AsRef<Path>, registry_path: impl AsRef<Path>) -> Result<()> {
    let socket_path = socket_path.as_ref().to_path_buf();
    let registry_path = registry_path.as_ref().to_path_buf();
    prepare_socket(&socket_path)?;
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("bind RPC socket {}", socket_path.display()))?;
    set_socket_permissions(&socket_path)?;
    loop {
        let (stream, _) = listener.accept().await.context("accept RPC client")?;
        let registry_path = registry_path.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_stream(stream, &registry_path).await {
                eprintln!("RPC client error: {error:#}");
            }
        });
    }
}

pub async fn serve_once(
    socket_path: impl AsRef<Path>,
    registry_path: impl AsRef<Path>,
) -> Result<()> {
    let socket_path = socket_path.as_ref().to_path_buf();
    let registry_path = registry_path.as_ref().to_path_buf();
    prepare_socket(&socket_path)?;
    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("bind RPC socket {}", socket_path.display()))?;
    set_socket_permissions(&socket_path)?;
    let (stream, _) = listener.accept().await.context("accept RPC client")?;
    handle_stream(stream, &registry_path).await
}

async fn handle_stream(stream: UnixStream, registry_path: &Path) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .context("read RPC request")?;
    let response = match serde_json::from_str::<Request>(&line) {
        Ok(request) => handle_request(request, registry_path).await,
        Err(error) => invalid_request(Value::Null, format!("invalid JSON-RPC request: {error}")),
    };
    let mut payload = serde_json::to_vec(&response)?;
    payload.push(b'\n');
    writer
        .write_all(&payload)
        .await
        .context("write RPC response")?;
    Ok(())
}

pub async fn handle_request(request: Request, registry_path: &Path) -> Response {
    if request.jsonrpc != "2.0" {
        return invalid_request(request.id, "jsonrpc must be \"2.0\"".into());
    }
    let result = match request.method.as_str() {
        "daemon.ping" => Ok(serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "service": "auto"
        })),
        "automation.list" => with_registry(registry_path, |registry| {
            Ok(serde_json::to_value(registry.list_automations()?)?)
        }),
        "automation.inspect" => {
            let id = match string_param(&request.params, "id") {
                Ok(id) => id,
                Err(error) => return invalid_params(request.id, error),
            };
            with_registry(registry_path, move |registry| {
                match registry.get_automation(&id)? {
                    Some(value) => Ok(serde_json::to_value(value)?),
                    None => anyhow::bail!("automation not found: {id}"),
                }
            })
        }
        "automation.policy_check" => {
            let id = match string_param(&request.params, "id") {
                Ok(id) => id,
                Err(error) => return invalid_params(request.id, error),
            };
            with_registry(registry_path, move |registry| {
                let automation = registry
                    .get_automation(&id)?
                    .with_context(|| format!("automation not found: {id}"))?;
                Ok(serde_json::to_value(crate::policy::check(&automation))?)
            })
        }
        "automation.pause" | "automation.resume" => {
            let id = match string_param(&request.params, "id") {
                Ok(id) => id,
                Err(error) => return invalid_params(request.id, error),
            };
            let desired = if request.method == "automation.pause" {
                RuntimeState::Paused
            } else {
                RuntimeState::Enabled
            };
            with_registry(registry_path, move |registry| {
                Ok(serde_json::to_value(
                    registry.transition_runtime_state(&id, desired)?,
                )?)
            })
        }
        "source.acknowledge_drift" => {
            let id = match string_param(&request.params, "id") {
                Ok(id) => id,
                Err(error) => return invalid_params(request.id, error),
            };
            with_registry(registry_path, move |registry| {
                Ok(serde_json::to_value(
                    registry.acknowledge_source_drift(&id)?,
                )?)
            })
        }
        "adoptions.list" => {
            let limit = match request.params.get("limit") {
                None => 100,
                Some(value) => match value.as_u64() {
                    Some(value) if (1..=500).contains(&value) => value as usize,
                    _ => {
                        return invalid_params(
                            request.id,
                            "params.limit must be an integer from 1 to 500".into(),
                        );
                    }
                },
            };
            with_registry(registry_path, |registry| {
                Ok(serde_json::to_value(registry.list_adoptions(limit)?)?)
            })
        }
        "inbox.list" => {
            let limit = match request.params.get("limit") {
                None => 100,
                Some(value) => match value.as_u64() {
                    Some(value) if (1..=500).contains(&value) => value as usize,
                    _ => {
                        return invalid_params(
                            request.id,
                            "params.limit must be an integer from 1 to 500".into(),
                        );
                    }
                },
            };
            with_registry(registry_path, move |registry| {
                Ok(serde_json::to_value(registry.list_inbox(limit)?)?)
            })
        }
        "adoption.inspect" => {
            let tx_id = match string_param(&request.params, "tx_id") {
                Ok(tx_id) => tx_id,
                Err(error) => return invalid_params(request.id, error),
            };
            with_registry(registry_path, move |registry| {
                let record = registry
                    .get_adoption(&tx_id)?
                    .with_context(|| format!("adoption transaction not found: {tx_id}"))?;
                Ok(serde_json::to_value(record)?)
            })
        }
        "automation.run" => {
            let id = match string_param(&request.params, "id") {
                Ok(id) => id,
                Err(error) => return invalid_params(request.id, error),
            };
            let allow_observed = request
                .params
                .get("allow_observed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match service::run_named(registry_path, &id, allow_observed).await {
                Ok(result) => serde_json::to_value(result).map_err(Into::into),
                Err(error) => Err(error),
            }
        }
        "run.cancel" => {
            let run_id = match string_param(&request.params, "run_id") {
                Ok(run_id) => run_id,
                Err(error) => return invalid_params(request.id, error),
            };
            match service::cancel_run(registry_path, &run_id) {
                Ok(()) => Ok(serde_json::json!({
                    "run_id": run_id,
                    "status": "cancel_requested"
                })),
                Err(error) => Err(error),
            }
        }
        "run.logs" => {
            let run_id = match string_param(&request.params, "run_id") {
                Ok(run_id) => run_id,
                Err(error) => return invalid_params(request.id, error),
            };
            with_registry(registry_path, move |registry| {
                let logs = registry
                    .get_run_logs(&run_id)?
                    .with_context(|| format!("run not found: {run_id}"))?;
                Ok(serde_json::to_value(logs)?)
            })
        }
        "approvals.list" => with_registry(registry_path, |registry| {
            Ok(serde_json::to_value(registry.list_approvals()?)?)
        }),
        "events.list" => {
            let limit = match request.params.get("limit") {
                None => 100,
                Some(value) => match value.as_u64() {
                    Some(value) if (1..=500).contains(&value) => value as usize,
                    _ => {
                        return invalid_params(
                            request.id,
                            "params.limit must be an integer from 1 to 500".into(),
                        );
                    }
                },
            };
            with_registry(registry_path, |registry| {
                Ok(serde_json::to_value(registry.list_events(limit)?)?)
            })
        }
        "runs.list" => {
            let limit = match request.params.get("limit") {
                None => 100,
                Some(value) => match value.as_u64() {
                    Some(value) if (1..=500).contains(&value) => value as usize,
                    _ => {
                        return invalid_params(
                            request.id,
                            "params.limit must be an integer from 1 to 500".into(),
                        );
                    }
                },
            };
            let automation_id = match request.params.get("automation_id") {
                None => None,
                Some(value) => match value.as_str() {
                    Some(value) if !value.trim().is_empty() => Some(value.to_owned()),
                    _ => {
                        return invalid_params(
                            request.id,
                            "params.automation_id must be a non-empty string".into(),
                        );
                    }
                },
            };
            with_registry(registry_path, move |registry| {
                Ok(serde_json::to_value(
                    registry.list_runs(limit, automation_id.as_deref())?,
                )?)
            })
        }
        "approval.approve" | "approval.reject" => {
            let id = match string_param(&request.params, "id") {
                Ok(id) => id,
                Err(error) => return invalid_params(request.id, error),
            };
            let actor = match request.params.get("actor") {
                None => "local-user".to_owned(),
                Some(value) => match value.as_str() {
                    Some(actor) if !actor.trim().is_empty() => actor.to_owned(),
                    _ => {
                        return invalid_params(
                            request.id,
                            "params.actor must be a non-empty string".into(),
                        );
                    }
                },
            };
            let state = if request.method == "approval.approve" {
                ApprovalState::Approved
            } else {
                ApprovalState::Rejected
            };
            with_registry(registry_path, move |registry| {
                registry.resolve_approval(&id, state, &actor)?;
                Ok(serde_json::to_value(
                    registry
                        .get_approval(&id)?
                        .context("resolved approval disappeared")?,
                )?)
            })
        }
        "metrics.list" => with_registry(registry_path, |registry| {
            Ok(serde_json::to_value(registry.list_metrics()?)?)
        }),
        _ => return method_not_found(request.id, request.method),
    };
    match result {
        Ok(result) => Response {
            jsonrpc: "2.0".into(),
            id: request.id,
            result: Some(result),
            error: None,
        },
        Err(error) => Response {
            jsonrpc: "2.0".into(),
            id: request.id,
            result: None,
            error: Some(ErrorObject {
                code: -32000,
                message: error.to_string(),
                data: None,
            }),
        },
    }
}

fn with_registry<F>(path: &Path, operation: F) -> Result<Value>
where
    F: FnOnce(&Registry) -> Result<Value>,
{
    let registry = Registry::open(path)?;
    operation(&registry)
}

fn string_param(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("params.{key} must be a string"))
}

fn invalid_request(id: Value, message: String) -> Response {
    Response {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(ErrorObject {
            code: -32600,
            message,
            data: None,
        }),
    }
}

fn invalid_params(id: Value, message: String) -> Response {
    Response {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(ErrorObject {
            code: -32602,
            message,
            data: None,
        }),
    }
}

fn method_not_found(id: Value, method: String) -> Response {
    Response {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(ErrorObject {
            code: -32601,
            message: format!("method not found: {method}"),
            data: None,
        }),
    }
}

fn prepare_socket(path: &Path) -> Result<()> {
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            anyhow::bail!("RPC socket already has a live server: {}", path.display());
        }
        std::fs::remove_file(path)
            .with_context(|| format!("remove stale RPC socket {}", path.display()))?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        set_directory_permissions(parent)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_socket_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_socket_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        ApprovalRequest, CommandSpec, DiscoveredSource, Ownership, Risk, StepSpec, Trigger,
    };
    use tempfile::tempdir;

    #[tokio::test]
    async fn ping_and_unknown_method_follow_json_rpc_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let ping = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(1),
                method: "daemon.ping".into(),
                params: Value::Null,
            },
            &path,
        )
        .await;
        assert_eq!(ping.result.unwrap()["protocol_version"], PROTOCOL_VERSION);
        let unknown = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(2),
                method: "nope".into(),
                params: Value::Null,
            },
            &path,
        )
        .await;
        assert_eq!(unknown.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn list_and_run_use_the_same_registry_service() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let registry = Registry::open(&path).unwrap();
        registry
            .save_automation(&crate::core::Automation {
                id: "rpc-a".into(),
                name: "rpc-a".into(),
                ownership: Ownership::Managed,
                steps: vec![StepSpec {
                    id: "echo".into(),
                    command: CommandSpec::argv("/bin/echo", ["rpc"]),
                    responses: None,
                    risk: crate::core::Risk::R0Read,
                }],
                ..Default::default()
            })
            .unwrap();
        let list = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(1),
                method: "automation.list".into(),
                params: Value::Null,
            },
            &path,
        )
        .await;
        assert_eq!(list.result.unwrap().as_array().unwrap().len(), 1);
        let policy = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(14),
                method: "automation.policy_check".into(),
                params: serde_json::json!({"id":"rpc-a"}),
            },
            &path,
        )
        .await;
        assert_eq!(policy.result.unwrap()["status"], "pass");
        let run = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(2),
                method: "automation.run".into(),
                params: serde_json::json!({"id":"rpc-a"}),
            },
            &path,
        )
        .await;
        assert_eq!(run.result.unwrap()["status"], "succeeded");
    }

    #[tokio::test]
    async fn lifecycle_rpc_pauses_and_resumes_owned_automation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        Registry::open(&path)
            .unwrap()
            .save_automation(&crate::core::Automation {
                id: "rpc-lifecycle".into(),
                name: "rpc-lifecycle".into(),
                ownership: Ownership::Managed,
                ..Default::default()
            })
            .unwrap();
        let paused = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(9),
                method: "automation.pause".into(),
                params: serde_json::json!({"id":"rpc-lifecycle"}),
            },
            &path,
        )
        .await;
        assert_eq!(paused.result.unwrap()["runtime_state"], "paused");
        let resumed = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(10),
                method: "automation.resume".into(),
                params: serde_json::json!({"id":"rpc-lifecycle"}),
            },
            &path,
        )
        .await;
        assert_eq!(resumed.result.unwrap()["runtime_state"], "enabled");
    }

    #[tokio::test]
    async fn drift_acknowledgement_rpc_updates_baseline_but_stays_paused() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let registry = Registry::open(&path).unwrap();
        let source = DiscoveredSource {
            source_id: "rpc-drift".into(),
            provider: "launchd".into(),
            native_id: "rpc-drift".into(),
            path: None,
            enabled: true,
            kind: "task".into(),
            fingerprint: "sha256:old".into(),
            command: Some(CommandSpec::argv("/bin/echo", ["old"])),
            trigger: Trigger::Manual,
            raw: "old".into(),
        };
        registry.reconcile_discovered_source(&source).unwrap();
        let mut adopted = source.as_observed_automation().unwrap();
        adopted.ownership = Ownership::Adopted;
        registry.save_automation(&adopted).unwrap();
        registry
            .reconcile_discovered_source(&DiscoveredSource {
                fingerprint: "sha256:new".into(),
                raw: "new".into(),
                ..source
            })
            .unwrap();
        let response = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(11),
                method: "source.acknowledge_drift".into(),
                params: serde_json::json!({"id":"rpc-drift"}),
            },
            &path,
        )
        .await;
        let acknowledged = response.result.unwrap();
        assert_eq!(acknowledged["runtime_state"], "paused");
        assert_eq!(acknowledged["fingerprint"], "sha256:new");
    }

    #[tokio::test]
    async fn adoption_rpc_exposes_bounded_journal_records() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let registry = Registry::open(&path).unwrap();
        registry
            .begin_adoption("tx-rpc", "launchd:test", r#"{"loaded":true}"#)
            .unwrap();
        let list = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(12),
                method: "adoptions.list".into(),
                params: serde_json::json!({"limit": 1}),
            },
            &path,
        )
        .await;
        assert_eq!(list.result.unwrap()[0]["tx_id"], "tx-rpc");
        let inspect = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(13),
                method: "adoption.inspect".into(),
                params: serde_json::json!({"tx_id": "tx-rpc"}),
            },
            &path,
        )
        .await;
        assert_eq!(inspect.result.unwrap()["snapshot"]["loaded"], true);
    }

    #[tokio::test]
    async fn inbox_rpc_returns_bounded_attention_items() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        Registry::open(&path)
            .unwrap()
            .save_approval(&ApprovalRequest::new(
                "rpc-inbox-approval",
                "external write",
                Risk::R2ExternalWrite,
                serde_json::json!({"path": "/tmp/example"}),
            ))
            .unwrap();
        let response = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(14),
                method: "inbox.list".into(),
                params: serde_json::json!({"limit": 1}),
            },
            &path,
        )
        .await;
        let items = response.result.unwrap();
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert_eq!(items[0]["kind"], "approval");
    }

    #[tokio::test]
    async fn approval_rpc_resolves_only_pending_requests() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        Registry::open(&path)
            .unwrap()
            .save_approval(&crate::core::ApprovalRequest::new(
                "rpc-approval",
                "codex.app_server.item/fileChange/requestApproval",
                crate::core::Risk::R1WorkspaceWrite,
                serde_json::json!({"path":"/tmp/example"}),
            ))
            .unwrap();
        let approved = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(3),
                method: "approval.approve".into(),
                params: serde_json::json!({"id":"rpc-approval","actor":"ui-test"}),
            },
            &path,
        )
        .await;
        assert_eq!(approved.result.unwrap()["state"], "approved");
        let rejected_again = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(4),
                method: "approval.reject".into(),
                params: serde_json::json!({"id":"rpc-approval"}),
            },
            &path,
        )
        .await;
        assert_eq!(rejected_again.error.unwrap().code, -32000);
    }

    #[tokio::test]
    async fn events_rpc_returns_newest_events_with_bounded_limit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let registry = Registry::open(&path).unwrap();
        for index in 0..3 {
            registry
                .append_event(&crate::core::Event {
                    run_id: None,
                    occurred_at: chrono::Utc::now(),
                    event_type: format!("test.{index}"),
                    payload: serde_json::json!({"index": index}),
                })
                .unwrap();
        }
        let response = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(5),
                method: "events.list".into(),
                params: serde_json::json!({"limit": 2}),
            },
            &path,
        )
        .await;
        let events = response.result.unwrap();
        assert_eq!(events.as_array().unwrap().len(), 2);
        assert_eq!(events[0]["event_type"], "test.2");
        let invalid = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(6),
                method: "events.list".into(),
                params: serde_json::json!({"limit": 0}),
            },
            &path,
        )
        .await;
        assert_eq!(invalid.error.unwrap().code, -32602);
    }

    #[tokio::test]
    async fn runs_rpc_returns_revision_snapshots_and_supports_automation_filter() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let registry = Registry::open(&path).unwrap();
        registry
            .save_automation(&crate::core::Automation {
                id: "run-rpc".into(),
                name: "run-rpc".into(),
                ownership: Ownership::Managed,
                steps: vec![StepSpec {
                    id: "echo".into(),
                    command: CommandSpec::argv("/bin/echo", ["snapshot"]),
                    responses: None,
                    risk: crate::core::Risk::R0Read,
                }],
                ..Default::default()
            })
            .unwrap();
        registry
            .record_run_start(
                "run-1",
                &registry.get_automation("run-rpc").unwrap().unwrap(),
                None,
            )
            .unwrap();
        let response = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(8),
                method: "runs.list".into(),
                params: serde_json::json!({"limit": 10, "automation_id": "run-rpc"}),
            },
            &path,
        )
        .await;
        let runs = response.result.unwrap();
        assert_eq!(runs.as_array().unwrap().len(), 1);
        assert_eq!(runs[0]["automation_snapshot"]["name"], "run-rpc");
    }

    #[tokio::test]
    async fn unix_socket_round_trip_uses_restricted_socket_file() {
        let dir = tempdir().unwrap();
        let socket = dir.path().join("nested").join("automationd.sock");
        let registry = dir.path().join("registry.sqlite3");
        let server_socket = socket.clone();
        let server_registry = registry.clone();
        let server =
            tokio::spawn(async move { serve_once(server_socket, server_registry).await.unwrap() });
        for _ in 0..50 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        let mut stream = UnixStream::connect(&socket).await.unwrap();
        stream
            .write_all(
                br#"{"jsonrpc":"2.0","id":7,"method":"daemon.ping","params":{}}
"#,
            )
            .await
            .unwrap();
        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .await
            .unwrap();
        let response: Response = serde_json::from_str(&response).unwrap();
        assert_eq!(response.result.unwrap()["service"], "auto");
        server.await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
