use crate::{
    codex::ensure_git_repository,
    core::{ApprovalRequest, ApprovalState, Risk, fingerprint_bytes},
    storage::Registry,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    process::Stdio,
    time::{Duration as StdDuration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::{Duration, sleep, timeout},
};
use uuid::Uuid;

pub const CLIENT_NAME: &str = "auto_control_plane";

pub type ApprovalDecisionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AppServerApprovalDecision>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppServerApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

impl AppServerApprovalDecision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::AcceptForSession => "acceptForSession",
            Self::Decline => "decline",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerApprovalRequest {
    pub id: Value,
    pub method: String,
    pub params: Value,
    pub risk: Risk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerApprovalOutcome {
    pub method: String,
    pub risk: Risk,
    pub decision: AppServerApprovalDecision,
}

pub trait ApprovalHandler: Send {
    fn decide<'a>(
        &'a mut self,
        request: &'a AppServerApprovalRequest,
    ) -> ApprovalDecisionFuture<'a>;
}

#[derive(Debug, Default)]
pub struct AutoDeclineApprovalHandler;

impl ApprovalHandler for AutoDeclineApprovalHandler {
    fn decide<'a>(
        &'a mut self,
        _request: &'a AppServerApprovalRequest,
    ) -> ApprovalDecisionFuture<'a> {
        Box::pin(async { Ok(AppServerApprovalDecision::Decline) })
    }
}

/// Bridges App Server approval requests to the local Registry Inbox.
///
/// The handler never grants a request above `max_risk`. Requests within the
/// bound remain pending until another process resolves them with the existing
/// `auto approve` or `auto reject` commands, or until the wait expires.
#[derive(Debug, Clone)]
pub struct RegistryApprovalHandler {
    registry_path: PathBuf,
    timeout: StdDuration,
    poll_interval: StdDuration,
    max_risk: Risk,
    announce: bool,
}

impl RegistryApprovalHandler {
    pub fn new(
        registry_path: impl Into<PathBuf>,
        timeout_seconds: u64,
        max_risk: Risk,
    ) -> Result<Self> {
        if timeout_seconds == 0 {
            anyhow::bail!("approval wait timeout must be greater than zero");
        }
        Ok(Self {
            registry_path: registry_path.into(),
            timeout: StdDuration::from_secs(timeout_seconds),
            poll_interval: StdDuration::from_millis(100),
            max_risk,
            announce: false,
        })
    }

    pub fn with_poll_interval(mut self, poll_interval: StdDuration) -> Result<Self> {
        if poll_interval.is_zero() {
            anyhow::bail!("approval poll interval must be greater than zero");
        }
        self.poll_interval = poll_interval;
        Ok(self)
    }

    pub fn with_announcement(mut self, announce: bool) -> Self {
        self.announce = announce;
        self
    }
}

impl ApprovalHandler for RegistryApprovalHandler {
    fn decide<'a>(
        &'a mut self,
        request: &'a AppServerApprovalRequest,
    ) -> ApprovalDecisionFuture<'a> {
        let registry_path = self.registry_path.clone();
        let timeout = self.timeout;
        let poll_interval = self.poll_interval;
        let max_risk = self.max_risk;
        let announce = self.announce;
        let request = request.clone();
        Box::pin(async move {
            let id = format!("approval_app_server_{}", Uuid::new_v4());
            let scope = serde_json::json!({
                "method": request.method,
                "request_id": request.id,
                "risk": request.risk,
                "params": redact_sensitive_fields(&request.params),
                "request_sha256": fingerprint_bytes(&serde_json::to_vec(&request.params)?),
            });
            let approval = ApprovalRequest::new(
                id.clone(),
                format!("codex.app_server.{}", request.method),
                request.risk,
                scope,
            );
            let registry = Registry::open(&registry_path)?;
            registry.save_approval(&approval)?;
            if announce {
                eprintln!(
                    "approval pending: {} ({}); run `auto approve {}` or `auto reject {}`",
                    id,
                    request.risk.label(),
                    id,
                    id
                );
            }
            if request.risk > max_risk {
                registry.resolve_approval(&id, ApprovalState::Rejected, "policy")?;
                return Ok(AppServerApprovalDecision::Decline);
            }
            let deadline = Instant::now() + timeout;
            loop {
                let current = Registry::open(&registry_path)?.get_approval(&id)?;
                match current.map(|value| value.state) {
                    Some(ApprovalState::Approved) => {
                        return Ok(AppServerApprovalDecision::Accept);
                    }
                    Some(ApprovalState::Rejected | ApprovalState::Expired) => {
                        return Ok(AppServerApprovalDecision::Decline);
                    }
                    Some(ApprovalState::Pending) | None if Instant::now() < deadline => {
                        sleep(poll_interval).await;
                    }
                    Some(ApprovalState::Pending) | None => {
                        Registry::open(&registry_path)?.resolve_approval(
                            &id,
                            ApprovalState::Expired,
                            "auto-timeout",
                        )?;
                        return Ok(AppServerApprovalDecision::Decline);
                    }
                }
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerConfig {
    pub codex_path: PathBuf,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub sandbox_policy: Value,
    pub approval_policy: String,
    pub timeout_seconds: u64,
}

impl Default for AppServerConfig {
    fn default() -> Self {
        Self {
            codex_path: PathBuf::from("codex"),
            cwd: PathBuf::from("."),
            model: None,
            sandbox_policy: serde_json::json!({"type": "readOnly"}),
            approval_policy: "on-request".into(),
            timeout_seconds: 30 * 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppServerRunResult {
    pub thread_id: String,
    pub turn_id: String,
    pub status: String,
    pub final_message: Option<String>,
    pub events: Vec<Value>,
    pub declined_approvals: u32,
    pub approval_outcomes: Vec<AppServerApprovalOutcome>,
}

pub struct AppServerClient {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
    pending_notifications: Vec<Value>,
    config: AppServerConfig,
}

impl AppServerClient {
    pub async fn connect(mut config: AppServerConfig) -> Result<Self> {
        config.cwd = config
            .cwd
            .canonicalize()
            .with_context(|| format!("resolve app-server cwd {}", config.cwd.display()))?;
        ensure_git_repository(&config.cwd)?;
        if config.timeout_seconds == 0 {
            anyhow::bail!("app-server timeout must be greater than zero");
        }
        let mut child = Command::new(&config.codex_path)
            .args(["app-server", "--listen", "stdio://"])
            .current_dir(&config.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("start {} app-server", config.codex_path.display()))?;
        let stdin = child.stdin.take().context("take app-server stdin")?;
        let stdout = child.stdout.take().context("take app-server stdout")?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 0,
            pending_notifications: Vec::new(),
            config,
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<()> {
        self.request("initialize", serde_json::json!({
            "clientInfo": {"name": CLIENT_NAME, "title": "Auto Control Plane", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"experimentalApi": false}
        })).await?;
        self.notify("initialized", serde_json::json!({})).await
    }

    pub async fn run_prompt(&mut self, prompt: &str) -> Result<AppServerRunResult> {
        let mut handler = AutoDeclineApprovalHandler;
        self.run_prompt_with_handler(prompt, &mut handler).await
    }

    pub async fn run_prompt_with_handler<H: ApprovalHandler>(
        &mut self,
        prompt: &str,
        handler: &mut H,
    ) -> Result<AppServerRunResult> {
        if prompt.trim().is_empty() {
            anyhow::bail!("app-server prompt must not be empty");
        }
        let mut thread_params = serde_json::json!({
            "cwd": self.config.cwd,
            "approvalPolicy": self.config.approval_policy,
            "sandbox": self.config.sandbox_policy,
            "serviceName": CLIENT_NAME,
        });
        if let Some(model) = &self.config.model {
            thread_params["model"] = Value::String(model.clone());
        }
        let thread_response = self
            .request_with_handler("thread/start", thread_params, handler)
            .await?;
        let thread_id = thread_response
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .context("app-server thread/start response missing thread.id")?
            .to_owned();
        let turn_response = self
            .request_with_handler(
                "turn/start",
                serde_json::json!({
                    "threadId": thread_id,
                    "input": [{"type": "text", "text": prompt}],
                    "cwd": self.config.cwd,
                    "approvalPolicy": self.config.approval_policy,
                    "sandboxPolicy": self.config.sandbox_policy,
                }),
                handler,
            )
            .await?;
        let turn_id = turn_response
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .context("app-server turn/start response missing turn.id")?
            .to_owned();
        let mut events = std::mem::take(&mut self.pending_notifications);
        let mut final_message = None;
        let mut declined_approvals = 0;
        let mut approval_outcomes = Vec::new();
        let status = loop {
            let message = self.read_message().await?;
            if let Some(outcome) = self.handle_server_request(&message, handler).await? {
                if outcome.decision == AppServerApprovalDecision::Decline {
                    declined_approvals += 1;
                }
                approval_outcomes.push(outcome);
                continue;
            }
            if message.get("method").and_then(Value::as_str) == Some("item/completed")
                && message.pointer("/params/item/type").and_then(Value::as_str)
                    == Some("agentMessage")
            {
                final_message = message
                    .pointer("/params/item/text")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            let is_completed = message.get("method").and_then(Value::as_str)
                == Some("turn/completed")
                && message.pointer("/params/turn/id").and_then(Value::as_str)
                    == Some(turn_id.as_str());
            if is_completed {
                let status = message
                    .pointer("/params/turn/status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                events.push(message);
                break status;
            }
            events.push(message);
        };
        Ok(AppServerRunResult {
            thread_id,
            turn_id,
            status,
            final_message,
            events,
            declined_approvals,
            approval_outcomes,
        })
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.child.kill().await.context("stop app-server")?;
        let _ = self.child.wait().await;
        Ok(())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let mut handler = AutoDeclineApprovalHandler;
        self.request_with_handler(method, params, &mut handler)
            .await
    }

    async fn request_with_handler<H: ApprovalHandler>(
        &mut self,
        method: &str,
        params: Value,
        handler: &mut H,
    ) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.write_message(serde_json::json!({"method": method, "id": id, "params": params}))
            .await?;
        loop {
            let message = self.read_message().await?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    anyhow::bail!("app-server {method} failed: {error}");
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            if self
                .handle_server_request(&message, handler)
                .await?
                .is_some()
            {
                continue;
            }
            self.pending_notifications.push(message);
        }
    }

    async fn handle_server_request<H: ApprovalHandler>(
        &mut self,
        message: &Value,
        handler: &mut H,
    ) -> Result<Option<AppServerApprovalOutcome>> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(None);
        };
        let Some(id) = message.get("id").cloned() else {
            return Ok(None);
        };
        if is_approval_request(message) {
            let request = approval_request_from_message(message)?;
            let decision = handler.decide(&request).await?;
            self.respond_decision(id, decision).await?;
            return Ok(Some(AppServerApprovalOutcome {
                method: method.to_owned(),
                risk: request.risk,
                decision,
            }));
        }
        self.respond_error(
            id,
            -32601,
            "auto does not implement this app-server server request",
        )
        .await?;
        Ok(None)
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write_message(serde_json::json!({"method": method, "params": params}))
            .await
    }

    async fn write_message(&mut self, message: Value) -> Result<()> {
        let mut payload = serde_json::to_vec(&message)?;
        payload.push(b'\n');
        self.stdin
            .write_all(&payload)
            .await
            .context("write app-server JSONL")?;
        self.stdin.flush().await.context("flush app-server JSONL")?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Value> {
        let line = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            self.stdout.next_line(),
        )
        .await
        .context("app-server response timeout")??
        .context("app-server closed stdout")?;
        serde_json::from_str(&line).with_context(|| format!("parse app-server JSONL: {line}"))
    }

    async fn respond_decision(
        &mut self,
        id: Value,
        decision: AppServerApprovalDecision,
    ) -> Result<()> {
        self.write_message(serde_json::json!({
            "id": id,
            "result": {"decision": decision.as_str()}
        }))
        .await
    }

    async fn respond_error(&mut self, id: Value, code: i32, message: &str) -> Result<()> {
        self.write_message(
            serde_json::json!({"id": id, "error": {"code": code, "message": message}}),
        )
        .await
    }
}

fn approval_request_from_message(message: &Value) -> Result<AppServerApprovalRequest> {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .context("app-server approval request is missing method")?;
    let id = message
        .get("id")
        .cloned()
        .context("app-server approval request is missing id")?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Ok(AppServerApprovalRequest {
        id,
        method: method.to_owned(),
        params,
        risk: approval_risk(method),
    })
}

fn approval_risk(method: &str) -> Risk {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            Risk::R1WorkspaceWrite
        }
        "mcpServer/elicitation/request" => Risk::R2ExternalWrite,
        "item/permissions/requestApproval" => Risk::R3SystemWrite,
        _ => Risk::R4Destructive,
    }
}

fn redact_sensitive_fields(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let lowered = key.to_ascii_lowercase();
                    let redacted = lowered.contains("token")
                        || lowered.contains("secret")
                        || lowered.contains("password")
                        || lowered.contains("api_key")
                        || lowered == "key";
                    (
                        key.clone(),
                        if redacted {
                            Value::String("[REDACTED]".into())
                        } else {
                            redact_sensitive_fields(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact_sensitive_fields).collect()),
        _ => value.clone(),
    }
}

fn is_approval_request(message: &Value) -> bool {
    matches!(
        message.get("method").and_then(Value::as_str),
        Some("item/commandExecution/requestApproval")
            | Some("item/fileChange/requestApproval")
            | Some("item/permissions/requestApproval")
            | Some("mcpServer/elicitation/request")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, process::Command as StdCommand};
    use tempfile::tempdir;

    #[test]
    fn default_app_server_is_read_only_and_requires_on_request_approval() {
        let config = AppServerConfig::default();
        assert_eq!(config.sandbox_policy["type"], "readOnly");
        assert_eq!(config.approval_policy, "on-request");
    }

    #[test]
    fn approval_requests_are_fail_closed() {
        assert!(is_approval_request(
            &serde_json::json!({"method":"item/commandExecution/requestApproval","id":1})
        ));
        assert!(is_approval_request(
            &serde_json::json!({"method":"item/fileChange/requestApproval","id":2})
        ));
        assert!(!is_approval_request(
            &serde_json::json!({"method":"item/completed"})
        ));
    }

    #[test]
    fn approval_requests_have_typed_risk_and_redacted_audit_scope() {
        let request = approval_request_from_message(&serde_json::json!({
            "id": 7,
            "method": "mcpServer/elicitation/request",
            "params": {"token": "do-not-store", "message": "confirm"}
        }))
        .unwrap();
        assert_eq!(request.risk, Risk::R2ExternalWrite);
        let redacted = redact_sensitive_fields(&request.params);
        assert_eq!(redacted["token"], "[REDACTED]");
        assert_eq!(redacted["message"], "confirm");
    }

    #[tokio::test]
    async fn registry_handler_rejects_requests_above_policy_and_records_them() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("registry.sqlite3");
        let request = approval_request_from_message(&serde_json::json!({
            "id": 8,
            "method": "item/permissions/requestApproval",
            "params": {"reason": "system access"}
        }))
        .unwrap();
        let mut handler = RegistryApprovalHandler::new(&path, 1, Risk::R1WorkspaceWrite)
            .unwrap()
            .with_poll_interval(StdDuration::from_millis(5))
            .unwrap();
        assert_eq!(
            handler.decide(&request).await.unwrap(),
            AppServerApprovalDecision::Decline
        );
        let approvals = Registry::open(&path).unwrap().list_approvals().unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].state, ApprovalState::Rejected);
        assert_eq!(approvals[0].actor.as_deref(), Some("policy"));
    }

    #[tokio::test]
    async fn registry_handler_waits_for_external_approval_and_accepts_it() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("registry.sqlite3");
        let request = approval_request_from_message(&serde_json::json!({
            "id": 9,
            "method": "item/fileChange/requestApproval",
            "params": {"reason": "update workspace"}
        }))
        .unwrap();
        let mut handler = RegistryApprovalHandler::new(&path, 2, Risk::R1WorkspaceWrite)
            .unwrap()
            .with_poll_interval(StdDuration::from_millis(5))
            .unwrap();
        let resolver_path = path.clone();
        let resolver = tokio::spawn(async move {
            let deadline = Instant::now() + StdDuration::from_secs(1);
            loop {
                if let Some(approval) = Registry::open(&resolver_path)
                    .unwrap()
                    .list_approvals()
                    .unwrap()
                    .into_iter()
                    .next()
                {
                    Registry::open(&resolver_path)
                        .unwrap()
                        .resolve_approval(&approval.id, ApprovalState::Approved, "test")
                        .unwrap();
                    return;
                }
                assert!(Instant::now() < deadline, "approval was not recorded");
                sleep(Duration::from_millis(5)).await;
            }
        });
        let decision = timeout(Duration::from_secs(1), handler.decide(&request))
            .await
            .unwrap()
            .unwrap();
        resolver.await.unwrap();
        assert_eq!(decision, AppServerApprovalDecision::Accept);
    }

    #[tokio::test]
    async fn stdio_client_completes_a_turn_after_registry_approval() {
        let directory = tempdir().unwrap();
        let repository = directory.path().join("repo");
        fs::create_dir(&repository).unwrap();
        assert!(
            StdCommand::new("git")
                .args(["init", "--quiet"])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );

        let fake_codex = directory.path().join("fake-codex");
        fs::write(
            &fake_codex,
            r##"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"id":1,"result":{}}'
      ;;
    *'"method":"thread/start"'*)
      printf '%s\n' '{"id":2,"result":{"thread":{"id":"thread-test"}}}'
      ;;
    *'"method":"turn/start"'*)
      printf '%s\n' '{"id":3,"result":{"turn":{"id":"turn-test"}}}'
      printf '%s\n' '{"method":"item/fileChange/requestApproval","id":99,"params":{"reason":"write workspace"}}'
      ;;
    *'"id":99,"result":{"decision":"accept"}'*)
      printf '%s\n' '{"method":"item/completed","params":{"item":{"type":"agentMessage","text":"approved"}}}'
      printf '%s\n' '{"method":"turn/completed","params":{"turn":{"id":"turn-test","status":"completed"}}}'
      ;;
  esac
done
"##,
        )
        .unwrap();
        fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o700)).unwrap();
        let registry_path = directory.path().join("registry.sqlite3");
        let config = AppServerConfig {
            codex_path: fake_codex,
            cwd: repository,
            timeout_seconds: 5,
            sandbox_policy: serde_json::json!({"type": "workspaceWrite"}),
            ..AppServerConfig::default()
        };
        let mut client = AppServerClient::connect(config).await.unwrap();
        let mut handler = RegistryApprovalHandler::new(&registry_path, 2, Risk::R1WorkspaceWrite)
            .unwrap()
            .with_poll_interval(StdDuration::from_millis(5))
            .unwrap();
        let resolver_path = registry_path.clone();
        let resolver = tokio::spawn(async move {
            let deadline = Instant::now() + StdDuration::from_secs(1);
            loop {
                if let Some(approval) = Registry::open(&resolver_path)
                    .unwrap()
                    .list_approvals()
                    .unwrap()
                    .into_iter()
                    .next()
                {
                    Registry::open(&resolver_path)
                        .unwrap()
                        .resolve_approval(&approval.id, ApprovalState::Approved, "test")
                        .unwrap();
                    return;
                }
                assert!(Instant::now() < deadline, "approval was not recorded");
                sleep(Duration::from_millis(5)).await;
            }
        });
        let result = client
            .run_prompt_with_handler("make the change", &mut handler)
            .await
            .unwrap();
        resolver.await.unwrap();
        client.shutdown().await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.final_message.as_deref(), Some("approved"));
        assert_eq!(result.approval_outcomes.len(), 1);
        assert_eq!(
            result.approval_outcomes[0].decision,
            AppServerApprovalDecision::Accept
        );
    }
}
