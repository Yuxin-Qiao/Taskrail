use crate::{
    core::{Automation, CommandSpec, Event, Ownership, RuntimeState, StepSpec, Trigger},
    discovery::{
        CronProvider, DiscoveryProvider, HomebrewProvider, LaunchdProvider, SystemdProvider,
        merge_homebrew_sources, same_native_path,
    },
    integrations::{
        GithubIntegration, HomebrewIntegration, Integration, IntegrationAction, MasIntegration,
        MoleIntegration, RcloneIntegration, ResticIntegration, SecurityIntegration,
        TopgradeIntegration,
    },
    service,
    storage::Registry,
};
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
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

/// Call the daemon's local JSON-RPC boundary from another Taskrail process,
/// such as the MCP adapter.
pub async fn call(
    socket_path: impl AsRef<Path>,
    method: impl Into<String>,
    params: Value,
) -> Result<Value> {
    let socket_path = socket_path.as_ref();
    let mut stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connect Taskrail daemon socket {}", socket_path.display()))?;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: Value::from(1),
        method: method.into(),
        params,
    };
    let mut payload = serde_json::to_vec(&request)?;
    payload.push(b'\n');
    stream
        .write_all(&payload)
        .await
        .context("write Taskrail daemon request")?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .context("read Taskrail daemon response")?;
    let response: Response = serde_json::from_str(&line).context("decode daemon response")?;
    if let Some(error) = response.error {
        anyhow::bail!("daemon RPC error {}: {}", error.code, error.message);
    }
    response
        .result
        .context("daemon response did not contain a result")
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
            "service": "taskrail"
        })),
        "daemon.status" => with_registry(registry_path, |registry| {
            let automations = registry.list_automations()?;
            let managed = automations
                .iter()
                .filter(|automation| automation.ownership == Ownership::Managed)
                .count();
            let adopted = automations
                .iter()
                .filter(|automation| automation.ownership == Ownership::Adopted)
                .count();
            let observed = automations
                .iter()
                .filter(|automation| automation.ownership == Ownership::Observed)
                .count();
            let paused = automations
                .iter()
                .filter(|automation| automation.runtime_state == RuntimeState::Paused)
                .count();
            Ok(serde_json::json!({
                "protocol_version": PROTOCOL_VERSION,
                "service": "taskrail",
                "automation_count": automations.len(),
                "managed_count": managed,
                "adopted_count": adopted,
                "observed_count": observed,
                "paused_count": paused,
            }))
        }),
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
        "automation.create" => {
            let params = match CreateAutomationParams::parse(&request.params) {
                Ok(params) => params,
                Err(error) => return invalid_params(request.id, error),
            };
            with_registry(registry_path, move |registry| {
                Ok(serde_json::to_value(create_automation(registry, params)?)?)
            })
        }
        "automation.scan" => {
            let source = match request.params.get("source") {
                None => "all".to_owned(),
                Some(value) => match value.as_str() {
                    Some(value) if !value.trim().is_empty() => value.to_owned(),
                    _ => {
                        return invalid_params(
                            request.id,
                            "params.source must be one of all, launchd, cron, systemd, homebrew"
                                .into(),
                        );
                    }
                },
            };
            let discovered = match scan_native_sources(&source) {
                Ok(discovered) => discovered,
                Err(error) => {
                    return Response {
                        jsonrpc: "2.0".into(),
                        id: request.id,
                        result: None,
                        error: Some(ErrorObject {
                            code: -32000,
                            message: error.to_string(),
                            data: None,
                        }),
                    };
                }
            };
            with_registry(registry_path, move |registry| {
                for item in &discovered {
                    registry.reconcile_discovered_source(item)?;
                }
                Ok(serde_json::to_value(discovered)?)
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
        "approval.request" => request_approval(registry_path, &request.params),
        "approval.execute" => execute_approved(registry_path, &request.params).await,
        "approvals.list" => {
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
                Ok(serde_json::to_value(registry.list_approvals(limit)?)?)
            })
        }
        "approval.approve" | "approval.reject" => {
            let id = match string_param(&request.params, "approval_id") {
                Ok(id) => id,
                Err(error) => return invalid_params(request.id, error),
            };
            let decision = if request.method == "approval.approve" {
                "approved"
            } else {
                "rejected"
            };
            with_registry(registry_path, move |registry| {
                let approval = registry.decide_approval(&id, decision)?;
                registry.append_event(&Event {
                    run_id: None,
                    occurred_at: Utc::now(),
                    event_type: format!("integration.approval.{decision}"),
                    payload: serde_json::json!({"approval_id": id, "status": decision}),
                })?;
                Ok(serde_json::to_value(approval)?)
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
        "integration.mole" => {
            integration_request(registry_path, &request.params, &MoleIntegration::default()).await
        }
        "integration.restic" => {
            integration_request(
                registry_path,
                &request.params,
                &ResticIntegration::default(),
            )
            .await
        }
        "integration.rclone" => {
            integration_request(
                registry_path,
                &request.params,
                &RcloneIntegration::default(),
            )
            .await
        }
        "integration.github" => {
            integration_request(
                registry_path,
                &request.params,
                &GithubIntegration::default(),
            )
            .await
        }
        "integration.homebrew" => {
            integration_request(
                registry_path,
                &request.params,
                &HomebrewIntegration::default(),
            )
            .await
        }
        "integration.mas" => {
            integration_request(registry_path, &request.params, &MasIntegration::default()).await
        }
        "integration.osv-scanner" => {
            integration_request(registry_path, &request.params, &SecurityIntegration::osv()).await
        }
        "integration.gitleaks" => {
            integration_request(
                registry_path,
                &request.params,
                &SecurityIntegration::gitleaks(),
            )
            .await
        }
        "integration.trivy" => {
            integration_request(
                registry_path,
                &request.params,
                &SecurityIntegration::trivy(),
            )
            .await
        }
        "integration.topgrade" => {
            integration_request(
                registry_path,
                &request.params,
                &TopgradeIntegration::default(),
            )
            .await
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

async fn integration_request(
    registry_path: &Path,
    params: &Value,
    integration: &dyn Integration,
) -> Result<Value> {
    let action_name = string_param(params, "action").map_err(anyhow::Error::msg)?;
    if matches!(action_name.as_str(), "detect" | "doctor") {
        return if action_name == "detect" {
            serde_json::to_value(integration.detect()).map_err(Into::into)
        } else {
            serde_json::to_value(integration.doctor()).map_err(Into::into)
        };
    }
    let parameters = params
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let action = IntegrationAction::with_parameters(action_name, parameters)?;
    let action = match params.get("approval_id").and_then(Value::as_str) {
        Some(approval_id) if !approval_id.trim().is_empty() => action.with_approval(approval_id),
        _ => action,
    };
    Ok(
        service::execute_integration(registry_path, integration, &action)
            .await?
            .semantic_value(),
    )
}

fn request_approval(registry_path: &Path, params: &Value) -> Result<Value> {
    let integration_name = string_param(params, "integration").map_err(anyhow::Error::msg)?;
    let action_name = string_param(params, "action").map_err(anyhow::Error::msg)?;
    let parameters = params
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let ttl_seconds = params
        .get("ttl_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(3600);
    let action = IntegrationAction::with_parameters(action_name, parameters)?;
    let result = match integration_name.as_str() {
        "mole" => service::request_integration_approval(
            registry_path,
            &MoleIntegration::default(),
            &action,
            ttl_seconds,
        )?,
        "restic" => service::request_integration_approval(
            registry_path,
            &ResticIntegration::default(),
            &action,
            ttl_seconds,
        )?,
        "rclone" => service::request_integration_approval(
            registry_path,
            &RcloneIntegration::default(),
            &action,
            ttl_seconds,
        )?,
        "homebrew" => service::request_integration_approval(
            registry_path,
            &HomebrewIntegration::default(),
            &action,
            ttl_seconds,
        )?,
        "topgrade" => service::request_integration_approval(
            registry_path,
            &TopgradeIntegration::default(),
            &action,
            ttl_seconds,
        )?,
        _ => anyhow::bail!("unsupported approval integration: {integration_name}"),
    };
    Ok(serde_json::to_value(result)?)
}

async fn execute_approved(registry_path: &Path, params: &Value) -> Result<Value> {
    let approval_id = string_param(params, "approval_id").map_err(anyhow::Error::msg)?;
    Ok(
        service::execute_approved_integration(registry_path, &approval_id)
            .await?
            .semantic_value(),
    )
}

fn with_registry<F>(path: &Path, operation: F) -> Result<Value>
where
    F: FnOnce(&Registry) -> Result<Value>,
{
    let registry = Registry::open(path)?;
    operation(&registry)
}

#[derive(Debug, Clone)]
struct CreateAutomationParams {
    id: String,
    name: String,
    executable: PathBuf,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    trigger: Trigger,
    timeout_seconds: u64,
}

impl CreateAutomationParams {
    fn parse(params: &Value) -> Result<Self, String> {
        let id = required_string(params, "id")?;
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&id)
            .to_owned();
        let executable = PathBuf::from(required_string(params, "executable")?);
        let args = match params.get("args") {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .ok_or_else(|| "params.args must be an array of strings".to_owned())?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "params.args must be an array of strings".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let command = CommandSpec {
            executable: executable.clone(),
            args: args.clone(),
            ..CommandSpec::default()
        };
        if command.invokes_shell() {
            return Err(
                "direct argv only: shell executables with -c/-e command strings are not accepted"
                    .into(),
            );
        }
        let cwd = params
            .get("cwd")
            .map(|value| {
                value
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .map(PathBuf::from)
                    .ok_or_else(|| "params.cwd must be a non-empty string".to_owned())
            })
            .transpose()?;
        let trigger_kind = params
            .get("trigger")
            .and_then(Value::as_str)
            .unwrap_or("manual");
        let trigger = match trigger_kind {
            "manual" => Trigger::Manual,
            "interval" => {
                let seconds = params
                    .get("interval_seconds")
                    .and_then(Value::as_u64)
                    .filter(|seconds| *seconds > 0)
                    .ok_or_else(|| {
                        "params.interval_seconds must be greater than zero for interval triggers"
                            .to_owned()
                    })?;
                Trigger::Interval { seconds }
            }
            "cron" => {
                let expression = required_string(params, "cron")?;
                let timezone = params
                    .get("timezone")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("local")
                    .to_owned();
                Trigger::Cron {
                    expression,
                    timezone,
                }
            }
            _ => return Err("params.trigger must be one of manual, interval, or cron".to_owned()),
        };
        let timeout_seconds = params
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(30 * 60);
        if timeout_seconds == 0 {
            return Err("params.timeout_seconds must be greater than zero".into());
        }
        Ok(Self {
            id,
            name,
            executable,
            args,
            cwd,
            trigger,
            timeout_seconds,
        })
    }
}

fn required_string(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("params.{key} must be a non-empty string"))
}

fn create_automation(registry: &Registry, params: CreateAutomationParams) -> Result<Automation> {
    if registry.get_automation(&params.id)?.is_some()
        || registry.get_automation(&params.name)?.is_some()
    {
        anyhow::bail!(
            "automation id or name already exists: {}",
            if registry.get_automation(&params.id)?.is_some() {
                params.id
            } else {
                params.name
            }
        );
    }
    let next_run_at = crate::scheduler::next_run(&params.trigger, Utc::now())?;
    let automation = Automation {
        id: params.id,
        name: params.name,
        ownership: Ownership::Managed,
        runtime_state: RuntimeState::Enabled,
        trigger: params.trigger,
        next_run_at,
        timeout_seconds: params.timeout_seconds,
        steps: vec![StepSpec {
            id: "command".into(),
            command: CommandSpec {
                executable: params.executable,
                args: params.args,
                cwd: params.cwd,
                ..CommandSpec::default()
            },
            responses: None,
        }],
        ..Automation::default()
    };
    registry.save_automation(&automation)?;
    registry.append_event(&Event {
        run_id: None,
        occurred_at: Utc::now(),
        event_type: "automation.created".into(),
        payload: serde_json::json!({
            "automation_id": automation.id,
            "source": "mcp",
        }),
    })?;
    Ok(automation)
}

fn scan_native_sources(source: &str) -> Result<Vec<crate::core::DiscoveredSource>> {
    let mut discovered = Vec::new();
    if matches!(source, "all" | "launchd") {
        discovered.extend(LaunchdProvider::default().scan()?);
    }
    if matches!(source, "all" | "cron") {
        discovered.extend(CronProvider::default().scan()?);
    }
    if matches!(source, "all" | "systemd") {
        discovered.extend(SystemdProvider::default().scan()?);
    }
    if matches!(source, "all" | "homebrew") {
        let homebrew = HomebrewProvider::default().scan()?;
        if source == "all" {
            let unmatched = merge_homebrew_sources(&mut discovered, homebrew);
            discovered.extend(unmatched);
        } else {
            let mut launchd = LaunchdProvider::default().scan()?;
            let unmatched = merge_homebrew_sources(&mut launchd, homebrew.clone());
            let mut related = homebrew
                .iter()
                .filter_map(|homebrew| {
                    launchd.iter().find(|native| {
                        native.provider == "launchd"
                            && same_native_path(native.path.as_deref(), homebrew.path.as_deref())
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            related.extend(unmatched);
            discovered.extend(related);
        }
    }
    if !matches!(source, "all" | "launchd" | "cron" | "systemd" | "homebrew") {
        anyhow::bail!(
            "unknown native source {source}; expected all, launchd, cron, systemd, or homebrew"
        );
    }
    Ok(discovered)
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
    use crate::core::{CommandSpec, DiscoveredSource, Ownership, StepSpec, Trigger};
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
    async fn daemon_status_reports_registry_ownership_counts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let registry = Registry::open(&path).unwrap();
        for (id, ownership, runtime_state) in [
            ("managed", Ownership::Managed, RuntimeState::Enabled),
            ("adopted", Ownership::Adopted, RuntimeState::Enabled),
            ("observed", Ownership::Observed, RuntimeState::Paused),
        ] {
            registry
                .save_automation(&crate::core::Automation {
                    id: id.into(),
                    name: id.into(),
                    ownership,
                    runtime_state,
                    ..Default::default()
                })
                .unwrap();
        }
        let status = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(1),
                method: "daemon.status".into(),
                params: Value::Null,
            },
            &path,
        )
        .await;
        let status = status.result.unwrap();
        assert_eq!(status["automation_count"], 3);
        assert_eq!(status["managed_count"], 1);
        assert_eq!(status["adopted_count"], 1);
        assert_eq!(status["observed_count"], 1);
        assert_eq!(status["paused_count"], 1);
    }

    #[tokio::test]
    async fn create_rpc_registers_managed_direct_argv_automation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let created = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(1),
                method: "automation.create".into(),
                params: serde_json::json!({
                    "id": "rpc-created",
                    "name": "RPC created",
                    "executable": "/bin/echo",
                    "args": ["hello"],
                    "trigger": "interval",
                    "interval_seconds": 3600,
                }),
            },
            &path,
        )
        .await;
        let automation = created.result.unwrap();
        assert_eq!(automation["ownership"], "managed");
        assert_eq!(automation["steps"][0]["command"]["shell"], false);
        assert_eq!(automation["trigger"]["kind"], "interval");
        assert!(automation["next_run_at"].is_string());

        let duplicate = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(2),
                method: "automation.create".into(),
                params: serde_json::json!({
                    "id": "rpc-created",
                    "executable": "/bin/echo",
                }),
            },
            &path,
        )
        .await;
        assert_eq!(duplicate.error.unwrap().code, -32000);

        let shell = handle_request(
            Request {
                jsonrpc: "2.0".into(),
                id: serde_json::json!(3),
                method: "automation.create".into(),
                params: serde_json::json!({
                    "id": "shell-attempt",
                    "executable": "/bin/sh",
                    "args": ["-c", "echo unsafe"],
                }),
            },
            &path,
        )
        .await;
        assert_eq!(shell.error.unwrap().code, -32602);
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
        assert_eq!(response.result.unwrap()["service"], "taskrail");
        server.await.unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
