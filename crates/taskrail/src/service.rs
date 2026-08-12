use crate::{
    core::{
        Automation, Event, IntegrationSpec, Ownership, RunResult, RuntimeState, StepSpec, Trigger,
    },
    executor,
    integrations::{
        DefaultPolicy, Integration, IntegrationAction, IntegrationResult, IntegrationStatus,
        PolicyDecision, PolicyEvaluator, ProcessOutput, ProcessStatus, VerificationResult,
        VerificationStatus,
    },
    scheduler,
    storage::Registry,
};
use anyhow::{Context, Result};
use chrono::Utc;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
    time::Instant,
};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, serde::Serialize)]
pub struct IntegrationExecution {
    pub plan: crate::integrations::ExecutionPlan,
    pub run: RunResult,
    pub result: IntegrationResult,
    pub verification: VerificationResult,
}

impl IntegrationExecution {
    /// Semantic response for CLI/RPC/MCP callers; raw output remains available
    /// only through the existing bounded `logs` read model.
    pub fn semantic_value(&self) -> serde_json::Value {
        serde_json::json!({
            "plan": self.plan,
            "run": {
                "run_id": self.run.run_id,
                "status": self.run.status,
                "exit_code": self.run.exit_code,
                "duration_ms": self.run.duration_ms,
            },
            "result": self.result,
            "verification": self.verification,
            "logs": "use taskrail logs with the returned run_id for bounded raw output",
        })
    }
}

/// Run a semantic integration through the existing Taskrail Run/Event path.
/// The adapter supplies meaning; the existing command executor supplies the
/// subprocess boundary.
pub async fn execute_integration(
    registry_path: &Path,
    integration: &dyn Integration,
    action: &IntegrationAction,
) -> Result<IntegrationExecution> {
    let plan = integration.plan(action)?;
    let plan_fingerprint = plan.fingerprint()?;
    match DefaultPolicy.evaluate(&plan) {
        PolicyDecision::Allowed => {}
        decision @ (PolicyDecision::RequiresApproval { .. } | PolicyDecision::Denied { .. }) => {
            let (event_type, reason) = match &decision {
                PolicyDecision::RequiresApproval { reason } => {
                    ("integration.approval_required", reason)
                }
                PolicyDecision::Denied { reason } => ("integration.denied", reason),
                PolicyDecision::Allowed => unreachable!(),
            };
            Registry::open(registry_path)?.append_event(&Event {
                run_id: None,
                occurred_at: Utc::now(),
                event_type: event_type.into(),
                payload: serde_json::json!({
                    "integration": plan.integration,
                    "action": plan.action,
                    "risk": plan.risk,
                    "dry_run": plan.dry_run,
                    "plan_fingerprint": plan_fingerprint,
                    "approval_id": action.approval_id,
                    "reason": reason,
                }),
            })?;
            match decision {
                PolicyDecision::RequiresApproval { reason } => {
                    let approval_id = action.approval_id.as_deref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "integration action held for approval: {reason}; request an approval first"
                        )
                    })?;
                    let registry = Registry::open(registry_path)?;
                    let approval = registry
                        .get_approval(approval_id)?
                        .ok_or_else(|| anyhow::anyhow!("approval not found: {approval_id}"))?;
                    if approval.integration != plan.integration.to_string()
                        || approval.action != plan.action
                        || approval.plan_fingerprint != plan_fingerprint
                    {
                        anyhow::bail!(
                            "approval does not match the requested integration plan: {approval_id}"
                        );
                    }
                    let consumed = registry.consume_approval(approval_id, &plan_fingerprint)?;
                    registry.append_event(&Event {
                        run_id: None,
                        occurred_at: Utc::now(),
                        event_type: "integration.approval.consumed".into(),
                        payload: serde_json::json!({
                            "approval_id": consumed.id,
                            "integration": consumed.integration,
                            "action": consumed.action,
                            "plan_fingerprint": consumed.plan_fingerprint,
                        }),
                    })?;
                }
                PolicyDecision::Denied { reason } => {
                    anyhow::bail!("integration action denied: {reason}");
                }
                PolicyDecision::Allowed => unreachable!(),
            }
        }
    }
    let mut automation = integration_automation(&plan, action)?;
    {
        let registry = Registry::open(registry_path)?;
        if let Some(existing) = registry.get_automation(&automation.id)?
            && !matches_same_integration_automation(&existing, &automation)
        {
            automation.id = format!("{}.{}", automation.id, Uuid::new_v4());
        }
        registry.save_automation(&automation)?;
        registry.append_event(&Event {
            run_id: None,
            occurred_at: Utc::now(),
            event_type: "integration.plan.created".into(),
            payload: serde_json::json!({
                "integration": plan.integration,
                "action": plan.action,
                "argv": plan.command.display(),
                "risk": plan.risk,
                "dry_run": plan.dry_run,
            }),
        })?;
    }
    let run_id = format!("run_{}", Uuid::new_v4());
    let (cancellation, _active_run) = register_active_run(&run_id);
    {
        let registry = Registry::open(registry_path)?;
        if !registry.try_record_run_start(&run_id, &automation, None)? {
            anyhow::bail!("integration action has an active run: {}", automation.name);
        }
        registry.append_event(&Event {
            run_id: Some(run_id.clone()),
            occurred_at: Utc::now(),
            event_type: "run.started".into(),
            payload: serde_json::json!({
                "automation": automation.name,
                "revision": automation.revision,
                "executor": "integration",
            }),
        })?;
    }
    let mut raw_run = if plan.plan_only {
        RunResult {
            run_id: run_id.clone(),
            status: "succeeded".into(),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
        }
    } else {
        match executor::execute_with_cancellation(&plan.command, plan.timeout_seconds, cancellation)
            .await
        {
            Ok(run) => run,
            Err(error) => RunResult {
                run_id: run_id.clone(),
                status: "failed".into(),
                exit_code: None,
                stdout: String::new(),
                stderr: error.to_string(),
                duration_ms: 0,
            },
        }
    };
    raw_run.run_id = run_id.clone();
    let mut run = raw_run.clone();
    run.stdout = redacted_integration_output(&plan, &run.stdout);
    run.stderr = redacted_integration_output(&plan, &run.stderr);
    let process_output = ProcessOutput {
        status: match raw_run.status.as_str() {
            "succeeded" => ProcessStatus::Succeeded,
            "timed_out" => ProcessStatus::TimedOut,
            "cancelled" => ProcessStatus::Cancelled,
            _ => ProcessStatus::Failed,
        },
        exit_code: raw_run.exit_code,
        stdout: raw_run.stdout.clone(),
        stderr: raw_run.stderr.clone(),
        duration_ms: raw_run.duration_ms,
    };
    let result = match integration.parse(action, process_output) {
        Ok(result) => result,
        Err(error) => {
            run.status = "failed".into();
            run.stderr = format!("{}\n[integration parse failed: {error}]", run.stderr);
            persist_integration_run(registry_path, &plan, &run)?;
            Registry::open(registry_path)?.append_event(&Event {
                run_id: Some(run.run_id.clone()),
                occurred_at: Utc::now(),
                event_type: "integration.parse.failed".into(),
                payload: serde_json::json!({
                    "integration": plan.integration,
                    "action": plan.action,
                    "error": error.to_string(),
                }),
            })?;
            return Err(error).context("parse integration output");
        }
    };
    let verification = match integration.verify(action, &result) {
        Ok(verification) => verification,
        Err(error) => {
            run.status = "failed".into();
            run.stderr = format!("{}\n[integration verification failed: {error}]", run.stderr);
            persist_integration_run(registry_path, &plan, &run)?;
            Registry::open(registry_path)?.append_event(&Event {
                run_id: Some(run.run_id.clone()),
                occurred_at: Utc::now(),
                event_type: "integration.verification.failed".into(),
                payload: serde_json::json!({"error": error.to_string()}),
            })?;
            return Err(error).context("verify integration result");
        }
    };
    run.status = if result.status == IntegrationStatus::Failed
        || verification.status == VerificationStatus::Failed
    {
        "failed".into()
    } else if result.status == IntegrationStatus::NeedsAttention {
        "needs_attention".into()
    } else {
        raw_run.status.clone()
    };
    persist_integration_run(registry_path, &plan, &run)?;
    let registry = Registry::open(registry_path)?;
    registry.append_event(&Event {
        run_id: Some(run.run_id.clone()),
        occurred_at: Utc::now(),
        event_type: "integration.result".into(),
        payload: serde_json::to_value(&result)?,
    })?;
    for (key, metric) in &result.metrics {
        registry.record_metric(&crate::core::Metric {
            id: format!("metric_integration_{}", Uuid::new_v4()),
            run_id: Some(run.run_id.clone()),
            key: key.clone(),
            value: metric.value,
            unit: metric.unit.clone(),
            source: format!("integration.{}", plan.integration),
            recorded_at: Utc::now(),
        })?;
    }
    if result.status == IntegrationStatus::NeedsAttention {
        registry.append_event(&Event {
            run_id: Some(run.run_id.clone()),
            occurred_at: Utc::now(),
            event_type: "integration.attention".into(),
            payload: serde_json::to_value(&result)?,
        })?;
    }
    if verification.status == VerificationStatus::Failed {
        registry.append_event(&Event {
            run_id: Some(run.run_id.clone()),
            occurred_at: Utc::now(),
            event_type: "integration.verification.failed".into(),
            payload: serde_json::to_value(&verification)?,
        })?;
    }
    Ok(IntegrationExecution {
        plan,
        run,
        result,
        verification,
    })
}

pub fn request_integration_approval(
    registry_path: &Path,
    integration: &dyn Integration,
    action: &IntegrationAction,
    ttl_seconds: u64,
) -> Result<crate::storage::StoredApproval> {
    if ttl_seconds == 0 || ttl_seconds > 7 * 24 * 60 * 60 {
        anyhow::bail!("approval TTL must be between 1 second and 7 days");
    }
    let action = sanitize_approval_action(integration, action)?;
    let plan = integration.plan(&action)?;
    let decision = DefaultPolicy.evaluate(&plan);
    if !matches!(decision, PolicyDecision::RequiresApproval { .. }) {
        anyhow::bail!("read-only integration actions do not require approval");
    }
    let reason = match decision {
        PolicyDecision::RequiresApproval { reason } => reason,
        _ => unreachable!(),
    };
    let approval_id = format!("approval_{}", Uuid::new_v4());
    let expires_at = (Utc::now() + chrono::Duration::seconds(ttl_seconds as i64)).to_rfc3339();
    let risk = serde_json::to_string(&plan.risk)?
        .trim_matches('"')
        .to_owned();
    let approval = Registry::open(registry_path)?.create_approval(
        &approval_id,
        plan.integration.as_str(),
        &plan.action,
        &plan.fingerprint()?,
        &serde_json::to_value(plan.redacted())?,
        &serde_json::to_value(action)?,
        &risk,
        &reason,
        &expires_at,
    )?;
    Registry::open(registry_path)?.append_event(&Event {
        run_id: None,
        occurred_at: Utc::now(),
        event_type: "integration.approval.requested".into(),
        payload: serde_json::json!({
            "approval_id": approval.id,
            "integration": approval.integration,
            "action": approval.action,
            "plan_fingerprint": approval.plan_fingerprint,
            "risk": approval.risk,
            "expires_at": approval.expires_at,
        }),
    })?;
    Ok(approval)
}

/// Create a durable, scheduler-owned automation for a typed integration.
///
/// Scheduled native actions are deliberately limited to read-only plans. A
/// recurring filesystem/system/network write must be surfaced as a fresh
/// approval decision for each execution instead of inheriting an old grant.
pub fn create_integration_automation(
    registry_path: &Path,
    integration: &dyn Integration,
    action: &IntegrationAction,
    id: String,
    name: Option<String>,
    trigger: Trigger,
) -> Result<Automation> {
    if id.trim().is_empty() {
        anyhow::bail!("integration automation id must not be empty");
    }
    let plan = integration.plan(action)?;
    if !matches!(DefaultPolicy.evaluate(&plan), PolicyDecision::Allowed) {
        anyhow::bail!(
            "scheduled integration actions must be read-only or dry-run; request approval for a one-time write"
        );
    }
    let integration_spec = IntegrationSpec::new(
        plan.integration.to_string(),
        plan.action.clone(),
        action.parameters.clone(),
    )?;
    let next_run_at = scheduler::next_run(&trigger, Utc::now())?;
    let display_name = name.unwrap_or_else(|| format!("{} · {}", plan.integration, plan.action));
    if display_name.trim().is_empty() {
        anyhow::bail!("integration automation name must not be empty");
    }
    let automation = Automation {
        id,
        name: display_name,
        ownership: Ownership::Managed,
        runtime_state: RuntimeState::Enabled,
        trigger,
        next_run_at,
        timeout_seconds: plan.timeout_seconds,
        steps: vec![StepSpec {
            id: "integration".into(),
            command: Default::default(),
            responses: None,
            integration: Some(integration_spec),
        }],
        ..Automation::default()
    };
    let registry = Registry::open(registry_path)?;
    if registry.get_automation(&automation.id)?.is_some()
        || registry.get_automation(&automation.name)?.is_some()
    {
        anyhow::bail!(
            "automation id or name already exists: {}",
            if registry.get_automation(&automation.id)?.is_some() {
                automation.id
            } else {
                automation.name
            }
        );
    }
    registry.save_automation(&automation)?;
    registry.append_event(&Event {
        run_id: None,
        occurred_at: Utc::now(),
        event_type: "integration.automation.created".into(),
        payload: serde_json::json!({
            "automation_id": automation.id,
            "integration": plan.integration,
            "action": plan.action,
            "trigger": automation.trigger,
            "risk": plan.risk,
            "dry_run": plan.dry_run,
        }),
    })?;
    Ok(automation)
}

/// Keep the durable approval request to the typed parameter surface of the
/// selected adapter. Unknown fields are rejected instead of being persisted;
/// this is especially important for callers that might accidentally put a
/// password or token in a generic JSON object.
fn sanitize_approval_action(
    integration: &dyn Integration,
    action: &IntegrationAction,
) -> Result<IntegrationAction> {
    let allowed = match integration.descriptor().id.as_str() {
        "mole" => ["dry_run", "limit"].as_slice(),
        "restic" => ["path", "repository_env", "password_env"].as_slice(),
        "rclone" => ["source", "destination", "dry_run", "config_env"].as_slice(),
        "homebrew" => ["file", "dry_run"].as_slice(),
        "topgrade" => [].as_slice(),
        id => anyhow::bail!("integration does not expose approval-capable parameters: {id}"),
    };
    let object = action
        .parameters
        .as_object()
        .context("integration parameters must be a JSON object")?;
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            anyhow::bail!(
                "unsupported parameter {key} for {} approval request",
                integration.descriptor().id
            );
        }
    }
    let parameters = object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    IntegrationAction::with_parameters(action.action.clone(), serde_json::Value::Object(parameters))
}

pub async fn execute_approved_integration(
    registry_path: &Path,
    approval_id: &str,
) -> Result<IntegrationExecution> {
    let approval = Registry::open(registry_path)?
        .get_approval(approval_id)?
        .with_context(|| format!("approval not found: {approval_id}"))?;
    if approval.status != "approved" {
        anyhow::bail!("approval is not executable: {}", approval.status);
    }
    let action: IntegrationAction =
        serde_json::from_value::<IntegrationAction>(approval.request.clone())
            .context("decode stored approval request")?
            .with_approval(approval_id);
    match approval.integration.as_str() {
        "mole" => {
            execute_integration(
                registry_path,
                &crate::integrations::MoleIntegration::default(),
                &action,
            )
            .await
        }
        "restic" => {
            execute_integration(
                registry_path,
                &crate::integrations::ResticIntegration::default(),
                &action,
            )
            .await
        }
        "rclone" => {
            execute_integration(
                registry_path,
                &crate::integrations::RcloneIntegration::default(),
                &action,
            )
            .await
        }
        "homebrew" => {
            execute_integration(
                registry_path,
                &crate::integrations::HomebrewIntegration::default(),
                &action,
            )
            .await
        }
        "topgrade" => {
            execute_integration(
                registry_path,
                &crate::integrations::TopgradeIntegration::default(),
                &action,
            )
            .await
        }
        integration => anyhow::bail!("unsupported approval integration: {integration}"),
    }
}

fn integration_automation(
    plan: &crate::integrations::ExecutionPlan,
    action: &IntegrationAction,
) -> Result<Automation> {
    Ok(Automation {
        id: format!("integration.{}.{}", plan.integration, plan.action),
        name: format!("{} · {}", plan.integration, plan.action),
        ownership: Ownership::Managed,
        runtime_state: RuntimeState::Enabled,
        trigger: Trigger::Manual,
        timeout_seconds: plan.timeout_seconds,
        steps: vec![crate::core::StepSpec {
            id: "integration".into(),
            command: Default::default(),
            responses: None,
            integration: Some(IntegrationSpec::new(
                plan.integration.to_string(),
                plan.action.clone(),
                action.parameters.clone(),
            )?),
        }],
        ..Automation::default()
    })
}

fn matches_same_integration_automation(left: &Automation, right: &Automation) -> bool {
    left.ownership == Ownership::Managed
        && left.source_id.is_none()
        && left.steps.len() == 1
        && right.steps.len() == 1
        && left.steps[0].command.executable.as_os_str().is_empty()
        && right.steps[0].command.executable.as_os_str().is_empty()
        && left.steps[0].integration.as_ref().map(|spec| {
            (
                spec.integration.as_str(),
                spec.action.as_str(),
                &spec.parameters,
            )
        }) == right.steps[0].integration.as_ref().map(|spec| {
            (
                spec.integration.as_str(),
                spec.action.as_str(),
                &spec.parameters,
            )
        })
}

fn integration_action_from_spec(spec: &IntegrationSpec) -> Result<IntegrationAction> {
    let validated = IntegrationSpec::new(
        spec.integration.clone(),
        spec.action.clone(),
        spec.parameters.clone(),
    )?;
    let action = IntegrationAction::with_parameters(validated.action, validated.parameters)?;
    Ok(match &spec.approval_id {
        Some(approval_id) => action.with_approval(approval_id.clone()),
        None => action,
    })
}

fn redacted_integration_output(plan: &crate::integrations::ExecutionPlan, output: &str) -> String {
    let mut output = if matches!(
        plan.integration.as_str(),
        "gitleaks" | "trivy" | "osv-scanner"
    ) {
        if output.is_empty() {
            String::new()
        } else {
            "[scanner output redacted; see normalized findings]".into()
        }
    } else {
        output.to_owned()
    };
    for environment in &plan.environment_refs {
        if let Ok(secret) = std::env::var(&environment.name)
            && !secret.is_empty()
        {
            output = output.replace(&secret, "[REDACTED]");
        }
    }
    output
}

fn persist_integration_run(
    registry_path: &Path,
    plan: &crate::integrations::ExecutionPlan,
    run: &RunResult,
) -> Result<()> {
    let registry = Registry::open(registry_path)?;
    registry.record_run_end(run)?;
    registry.append_event(&Event {
        run_id: Some(run.run_id.clone()),
        occurred_at: Utc::now(),
        event_type: format!(
            "executor.integration.{}",
            if run.status == "succeeded" {
                "completed"
            } else {
                "failed"
            }
        ),
        payload: serde_json::json!({
            "integration": plan.integration,
            "action": plan.action,
            "exit_code": run.exit_code,
        }),
    })?;
    registry.append_event(&Event {
        run_id: Some(run.run_id.clone()),
        occurred_at: Utc::now(),
        event_type: format!("run.{}", run.status),
        payload: serde_json::json!({
            "exit_code": run.exit_code,
            "duration_ms": run.duration_ms,
        }),
    })?;
    Ok(())
}

async fn execute_scheduled_integration_step(
    registry_path: &Path,
    spec: &IntegrationSpec,
    run_id: &str,
    timeout_seconds: u64,
    cancellation: watch::Receiver<bool>,
) -> Result<RunResult> {
    let integrations = crate::integrations::built_in_registry()?;
    let integration_id = crate::integrations::IntegrationId::new(spec.integration.clone())?;
    let integration = integrations
        .get(&integration_id)
        .with_context(|| format!("integration not registered: {}", spec.integration))?;
    let action = integration_action_from_spec(spec)?;
    let plan = integration.plan(&action)?;
    let plan_fingerprint = plan.fingerprint()?;

    match DefaultPolicy.evaluate(&plan) {
        PolicyDecision::Allowed => {}
        PolicyDecision::RequiresApproval { reason } => {
            Registry::open(registry_path)?.append_event(&Event {
                run_id: Some(run_id.to_owned()),
                occurred_at: Utc::now(),
                event_type: "integration.approval_required".into(),
                payload: serde_json::json!({
                    "integration": plan.integration,
                    "action": plan.action,
                    "risk": plan.risk,
                    "plan_fingerprint": plan_fingerprint,
                    "approval_id": action.approval_id,
                    "reason": reason,
                }),
            })?;
            let Some(approval_id) = action.approval_id.as_deref() else {
                return Ok(RunResult {
                    run_id: run_id.to_owned(),
                    status: "failed".into(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("integration action held for approval: {reason}"),
                    duration_ms: 0,
                });
            };
            let registry = Registry::open(registry_path)?;
            let approval = registry
                .get_approval(approval_id)?
                .with_context(|| format!("approval not found: {approval_id}"))?;
            if approval.integration != plan.integration.to_string()
                || approval.action != plan.action
                || approval.plan_fingerprint != plan_fingerprint
            {
                return Ok(RunResult {
                    run_id: run_id.to_owned(),
                    status: "failed".into(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!(
                        "approval does not match the requested integration plan: {approval_id}"
                    ),
                    duration_ms: 0,
                });
            }
            let consumed = registry.consume_approval(approval_id, &plan_fingerprint)?;
            registry.append_event(&Event {
                run_id: Some(run_id.to_owned()),
                occurred_at: Utc::now(),
                event_type: "integration.approval.consumed".into(),
                payload: serde_json::json!({
                    "approval_id": consumed.id,
                    "integration": consumed.integration,
                    "action": consumed.action,
                    "plan_fingerprint": consumed.plan_fingerprint,
                }),
            })?;
        }
        PolicyDecision::Denied { reason } => {
            Registry::open(registry_path)?.append_event(&Event {
                run_id: Some(run_id.to_owned()),
                occurred_at: Utc::now(),
                event_type: "integration.denied".into(),
                payload: serde_json::json!({
                    "integration": plan.integration,
                    "action": plan.action,
                    "reason": reason,
                }),
            })?;
            return Ok(RunResult {
                run_id: run_id.to_owned(),
                status: "failed".into(),
                exit_code: None,
                stdout: String::new(),
                stderr: format!("integration action denied: {reason}"),
                duration_ms: 0,
            });
        }
    }

    let raw_run = if plan.plan_only {
        RunResult {
            run_id: run_id.to_owned(),
            status: "succeeded".into(),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
        }
    } else {
        match executor::execute_with_cancellation(&plan.command, timeout_seconds, cancellation)
            .await
        {
            Ok(result) => result,
            Err(error) => RunResult {
                run_id: run_id.to_owned(),
                status: "failed".into(),
                exit_code: None,
                stdout: String::new(),
                stderr: error.to_string(),
                duration_ms: 0,
            },
        }
    };
    let process_output = ProcessOutput {
        status: match raw_run.status.as_str() {
            "succeeded" => ProcessStatus::Succeeded,
            "timed_out" => ProcessStatus::TimedOut,
            "cancelled" => ProcessStatus::Cancelled,
            _ => ProcessStatus::Failed,
        },
        exit_code: raw_run.exit_code,
        stdout: raw_run.stdout.clone(),
        stderr: raw_run.stderr.clone(),
        duration_ms: raw_run.duration_ms,
    };
    let result = match integration.parse(&action, process_output) {
        Ok(result) => result,
        Err(error) => {
            Registry::open(registry_path)?.append_event(&Event {
                run_id: Some(run_id.to_owned()),
                occurred_at: Utc::now(),
                event_type: "integration.parse.failed".into(),
                payload: serde_json::json!({
                    "integration": plan.integration,
                    "action": plan.action,
                    "error": error.to_string(),
                }),
            })?;
            return Ok(RunResult {
                run_id: run_id.to_owned(),
                status: "failed".into(),
                exit_code: raw_run.exit_code,
                stdout: redacted_integration_output(&plan, &raw_run.stdout),
                stderr: redacted_integration_output(&plan, &raw_run.stderr),
                duration_ms: raw_run.duration_ms,
            });
        }
    };
    let verification = integration.verify(&action, &result)?;
    let registry = Registry::open(registry_path)?;
    registry.append_event(&Event {
        run_id: Some(run_id.to_owned()),
        occurred_at: Utc::now(),
        event_type: "integration.result".into(),
        payload: serde_json::to_value(&result)?,
    })?;
    for (key, metric) in &result.metrics {
        registry.record_metric(&crate::core::Metric {
            id: format!("metric_integration_{}", Uuid::new_v4()),
            run_id: Some(run_id.to_owned()),
            key: key.clone(),
            value: metric.value,
            unit: metric.unit.clone(),
            source: format!("integration.{}", plan.integration),
            recorded_at: Utc::now(),
        })?;
    }
    if result.status == IntegrationStatus::NeedsAttention {
        registry.append_event(&Event {
            run_id: Some(run_id.to_owned()),
            occurred_at: Utc::now(),
            event_type: "integration.attention".into(),
            payload: serde_json::to_value(&result)?,
        })?;
    }
    if verification.status == VerificationStatus::Failed {
        registry.append_event(&Event {
            run_id: Some(run_id.to_owned()),
            occurred_at: Utc::now(),
            event_type: "integration.verification.failed".into(),
            payload: serde_json::to_value(&verification)?,
        })?;
    }
    let status = if result.status == IntegrationStatus::Failed
        || verification.status == VerificationStatus::Failed
    {
        "failed"
    } else if result.status == IntegrationStatus::NeedsAttention {
        "needs_attention"
    } else if raw_run.status == "succeeded" {
        "succeeded"
    } else {
        raw_run.status.as_str()
    };
    Ok(RunResult {
        run_id: run_id.to_owned(),
        status: status.into(),
        exit_code: raw_run.exit_code,
        stdout: redacted_integration_output(&plan, &raw_run.stdout),
        stderr: redacted_integration_output(&plan, &raw_run.stderr),
        duration_ms: raw_run.duration_ms,
    })
}

static ACTIVE_RUNS: OnceLock<Mutex<HashMap<String, watch::Sender<bool>>>> = OnceLock::new();

fn active_runs() -> &'static Mutex<HashMap<String, watch::Sender<bool>>> {
    ACTIVE_RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct ActiveRunGuard {
    run_id: String,
}

impl Drop for ActiveRunGuard {
    fn drop(&mut self) {
        if let Ok(mut active_runs) = active_runs().lock() {
            active_runs.remove(&self.run_id);
        }
    }
}

fn register_active_run(run_id: &str) -> (watch::Receiver<bool>, ActiveRunGuard) {
    let (sender, receiver) = watch::channel(false);
    active_runs()
        .lock()
        .expect("active run registry mutex poisoned")
        .insert(run_id.to_owned(), sender);
    (
        receiver,
        ActiveRunGuard {
            run_id: run_id.to_owned(),
        },
    )
}

pub async fn run_named(
    registry_path: impl AsRef<Path>,
    id: &str,
    allow_observed: bool,
) -> Result<RunResult> {
    let registry_path = registry_path.as_ref().to_path_buf();
    let automation = {
        let registry = Registry::open(&registry_path)?;
        registry
            .get_automation(id)?
            .with_context(|| format!("automation not found: {id}"))?
    };
    if automation.ownership == Ownership::Observed && !allow_observed {
        anyhow::bail!(
            "{} is observed-only; use --allow-observed for an explicit manual run",
            automation.name
        );
    }
    execute_automation(&registry_path, &automation).await
}

pub async fn execute_automation(
    registry_path: &Path,
    automation: &Automation,
) -> Result<RunResult> {
    execute_automation_at(registry_path, automation, None).await
}

pub fn recover_interrupted_runs(registry_path: impl AsRef<Path>) -> Result<u32> {
    let recovered = Registry::open(registry_path)?.recover_running_runs()?;
    Ok(recovered.len() as u32)
}

pub fn cancel_run(registry_path: impl AsRef<Path>, run_id: &str) -> Result<()> {
    let sender = active_runs()
        .lock()
        .expect("active run registry mutex poisoned")
        .get(run_id)
        .cloned()
        .with_context(|| format!("run is not active in this daemon: {run_id}"))?;
    if !*sender.borrow() {
        sender
            .send(true)
            .map_err(|_| anyhow::anyhow!("run cancellation channel closed: {run_id}"))?;
        Registry::open(registry_path)?.append_event(&Event {
            run_id: Some(run_id.to_owned()),
            occurred_at: Utc::now(),
            event_type: "run.cancel_requested".into(),
            payload: serde_json::json!({"reason": "operator_request"}),
        })?;
    }
    Ok(())
}

async fn execute_automation_at(
    registry_path: &Path,
    automation: &Automation,
    scheduled_at: Option<chrono::DateTime<Utc>>,
) -> Result<RunResult> {
    if automation.steps.is_empty() {
        anyhow::bail!("automation {} has no executable steps", automation.name);
    }
    if automation.retry.max_attempts == 0 {
        anyhow::bail!(
            "automation {} has zero attempts configured",
            automation.name
        );
    }
    if automation.retry.initial_backoff_seconds > automation.retry.max_backoff_seconds {
        anyhow::bail!(
            "automation {} has an invalid retry backoff",
            automation.name
        );
    }
    for step in &automation.steps {
        let has_command = !step.command.executable.as_os_str().is_empty();
        let has_responses = step.responses.is_some();
        let has_integration = step.integration.is_some();
        let executor_count = [has_command, has_responses, has_integration]
            .into_iter()
            .filter(|value| *value)
            .count();
        if executor_count == 0 {
            anyhow::bail!("step {} has no configured executor", step.id);
        }
        if executor_count > 1 {
            anyhow::bail!("step {} configures multiple executors", step.id);
        }
        if let Some(spec) = &step.integration {
            if spec.integration.trim().is_empty() || spec.action.trim().is_empty() {
                anyhow::bail!(
                    "step {} has an incomplete integration specification",
                    step.id
                );
            }
            if !spec.parameters.is_object() {
                anyhow::bail!(
                    "step {} integration parameters must be a JSON object",
                    step.id
                );
            }
        }
        if let Some(spec) = &step.responses
            && spec.prompt.trim().is_empty()
        {
            anyhow::bail!("step {} has an empty Responses API prompt", step.id);
        }
    }
    let run_id = format!("run_{}", Uuid::new_v4());
    let (cancellation, _active_run) = register_active_run(&run_id);
    {
        let registry = Registry::open(registry_path)?;
        if !registry.try_record_run_start(&run_id, automation, scheduled_at)? {
            registry.append_event(&Event {
                run_id: None,
                occurred_at: Utc::now(),
                event_type: "run.admission_rejected".into(),
                payload: serde_json::json!({
                    "automation_id": automation.id,
                    "reason": "forbid_overlap",
                }),
            })?;
            anyhow::bail!(
                "automation {} has an active run and forbids overlap",
                automation.name
            );
        }
        registry.append_event(&Event {
            run_id: Some(run_id.clone()),
            occurred_at: Utc::now(),
            event_type: "run.started".into(),
            payload: serde_json::json!({"automation": automation.name, "revision": automation.revision}),
        })?;
    }
    let mut final_result = RunResult {
        run_id: run_id.clone(),
        status: "succeeded".into(),
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        duration_ms: 0,
    };
    'steps: for step in &automation.steps {
        for attempt in 1..=automation.retry.max_attempts {
            let executor_kind = if step.integration.is_some() {
                "integration"
            } else if step.responses.is_some() {
                "responses"
            } else {
                "command"
            };
            {
                let registry = Registry::open(registry_path)?;
                registry.append_event(&Event {
                    run_id: Some(run_id.clone()),
                    occurred_at: Utc::now(),
                    event_type: format!("executor.{executor_kind}.started"),
                    payload: serde_json::json!({
                        "step": step.id,
                        "argv": match executor_kind {
                            "command" => serde_json::Value::String(step.command.display()),
                            "integration" => step.integration.as_ref().map_or(
                                serde_json::Value::Null,
                                |spec| serde_json::json!({
                                    "integration": spec.integration,
                                    "action": spec.action,
                                }),
                            ),
                            _ => serde_json::Value::Null,
                        },
                        "model": step.responses.as_ref().and_then(|spec| spec.model.clone()),
                        "attempt": attempt,
                    }),
                })?;
            }
            let mut responses_usage = None;
            let mut result = if let Some(spec) = &step.responses {
                let started_at = Instant::now();
                match crate::responses::ResponsesConfig::from_spec(spec, automation.timeout_seconds)
                {
                    Ok(config) => {
                        match config
                            .execute_with_cancellation(&spec.prompt, cancellation.clone())
                            .await
                        {
                            Ok(response) => {
                                responses_usage = response.usage;
                                RunResult {
                                    run_id: run_id.clone(),
                                    status: "succeeded".into(),
                                    exit_code: Some(0),
                                    stdout: response.output_text,
                                    stderr: String::new(),
                                    duration_ms: started_at.elapsed().as_millis(),
                                }
                            }
                            Err(error) => RunResult {
                                run_id: run_id.clone(),
                                status: if *cancellation.borrow() {
                                    "cancelled"
                                } else {
                                    "failed"
                                }
                                .into(),
                                exit_code: None,
                                stdout: String::new(),
                                stderr: error.to_string(),
                                duration_ms: started_at.elapsed().as_millis(),
                            },
                        }
                    }
                    Err(error) => RunResult {
                        run_id: run_id.clone(),
                        status: "failed".into(),
                        exit_code: None,
                        stdout: String::new(),
                        stderr: error.to_string(),
                        duration_ms: started_at.elapsed().as_millis(),
                    },
                }
            } else if let Some(spec) = &step.integration {
                match execute_scheduled_integration_step(
                    registry_path,
                    spec,
                    &run_id,
                    automation.timeout_seconds,
                    cancellation.clone(),
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => RunResult {
                        run_id: run_id.clone(),
                        status: "failed".into(),
                        exit_code: None,
                        stdout: String::new(),
                        stderr: error.to_string(),
                        duration_ms: 0,
                    },
                }
            } else {
                match executor::execute_with_cancellation(
                    &step.command,
                    automation.timeout_seconds,
                    cancellation.clone(),
                )
                .await
                {
                    Ok(result) => result,
                    Err(error) => RunResult {
                        run_id: run_id.clone(),
                        status: "failed".into(),
                        exit_code: None,
                        stdout: String::new(),
                        stderr: error.to_string(),
                        duration_ms: 0,
                    },
                }
            };
            result.run_id = run_id.clone();
            final_result.stdout.push_str(&result.stdout);
            final_result.stderr.push_str(&result.stderr);
            final_result.duration_ms += result.duration_ms;
            final_result.exit_code = result.exit_code;
            if let Some(usage) = responses_usage {
                let registry = Registry::open(registry_path)?;
                for (key, value) in [
                    ("input_tokens", usage.input_tokens),
                    ("output_tokens", usage.output_tokens),
                    ("total_tokens", usage.total_tokens),
                ] {
                    if let Some(value) = value {
                        registry.record_metric(&crate::core::Metric {
                            id: format!("metric_responses_{}", Uuid::new_v4()),
                            run_id: Some(run_id.clone()),
                            key: key.into(),
                            value: value as f64,
                            unit: "tokens".into(),
                            source: "responses.api".into(),
                            recorded_at: Utc::now(),
                        })?;
                    }
                }
            }
            if result.status == "succeeded" {
                let registry = Registry::open(registry_path)?;
                registry.append_event(&Event {
                    run_id: Some(run_id.clone()),
                    occurred_at: Utc::now(),
                    event_type: format!("executor.{executor_kind}.completed"),
                    payload: serde_json::json!({
                        "step": step.id,
                        "exit_code": result.exit_code,
                        "attempt": attempt,
                    }),
                })?;
                break;
            }
            {
                let registry = Registry::open(registry_path)?;
                registry.append_event(&Event {
                    run_id: Some(run_id.clone()),
                    occurred_at: Utc::now(),
                    event_type: if result.status == "cancelled" {
                        format!("executor.{executor_kind}.cancelled")
                    } else {
                        format!("executor.{executor_kind}.failed")
                    },
                    payload: serde_json::json!({
                        "step": step.id,
                        "attempt": attempt,
                        "status": result.status,
                        "exit_code": result.exit_code,
                    }),
                })?;
            }
            let retryable = matches!(result.status.as_str(), "failed" | "timed_out")
                && attempt < automation.retry.max_attempts;
            if !retryable {
                final_result.status = result.status;
                break 'steps;
            }
            let backoff_seconds = retry_backoff_seconds(&automation.retry, attempt);
            Registry::open(registry_path)?.append_event(&Event {
                run_id: Some(run_id.clone()),
                occurred_at: Utc::now(),
                event_type: format!("executor.{executor_kind}.retrying"),
                payload: serde_json::json!({
                    "step": step.id,
                    "attempt": attempt,
                    "next_attempt": attempt + 1,
                    "backoff_seconds": backoff_seconds,
                    "status": result.status,
                }),
            })?;
            if wait_for_retry(backoff_seconds, cancellation.clone()).await {
                final_result.status = "cancelled".into();
                break 'steps;
            }
        }
    }
    let registry = Registry::open(registry_path)?;
    registry.record_run_end(&final_result)?;
    registry.append_event(&Event {
        run_id: Some(run_id),
        occurred_at: Utc::now(),
        event_type: format!("run.{}", final_result.status),
        payload: serde_json::json!({"exit_code": final_result.exit_code, "duration_ms": final_result.duration_ms}),
    })?;
    Ok(final_result)
}

fn retry_backoff_seconds(config: &crate::core::RetryPolicy, attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(63);
    let multiplier = 1_u64.checked_shl(exponent).unwrap_or(u64::MAX);
    config
        .initial_backoff_seconds
        .saturating_mul(multiplier)
        .min(config.max_backoff_seconds)
}

async fn wait_for_retry(backoff_seconds: u64, mut cancellation: watch::Receiver<bool>) -> bool {
    if *cancellation.borrow() {
        return true;
    }
    if backoff_seconds == 0 {
        return false;
    }
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_seconds)) => false,
        changed = cancellation.changed() => changed.is_err() || *cancellation.borrow(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerPass {
    pub due: u32,
    pub failed: u32,
}

pub async fn scheduled_pass(registry_path: impl AsRef<Path>) -> Result<SchedulerPass> {
    let registry_path = registry_path.as_ref().to_path_buf();
    let now = Utc::now();
    let automations = Registry::open(&registry_path)?.list_automations()?;
    let mut pass = SchedulerPass { due: 0, failed: 0 };
    for automation in automations {
        if !matches!(
            automation.ownership,
            Ownership::Adopted | Ownership::Managed
        ) || automation.runtime_state != RuntimeState::Enabled
            || matches!(automation.trigger, Trigger::Manual)
        {
            continue;
        }
        let scheduled = automation.next_run_at.unwrap_or(now);
        let due_runs =
            scheduler::due_runs(&automation.trigger, scheduled, now, automation.misfire)?;
        let running = Registry::open(&registry_path)?.count_running_runs(&automation.id)?;
        if !scheduler::may_start(automation.concurrency, running) {
            continue;
        }
        if let Some(max_age_seconds) = automation.misfire_max_age_seconds {
            let age_seconds = now
                .signed_duration_since(scheduled)
                .num_seconds()
                .try_into()
                .unwrap_or(u64::MAX);
            if scheduled <= now && age_seconds > max_age_seconds {
                Registry::open(&registry_path)?.append_event(&Event {
                    run_id: None,
                    occurred_at: now,
                    event_type: "scheduler.misfire_expired".into(),
                    payload: serde_json::json!({
                        "automation_id": automation.id,
                        "scheduled_at": scheduled.to_rfc3339(),
                        "age_seconds": age_seconds,
                        "max_age_seconds": max_age_seconds,
                    }),
                })?;
                let mut updated = automation;
                updated.next_run_at = scheduler::next_run(&updated.trigger, now)?;
                Registry::open(&registry_path)?.save_automation(&updated)?;
                continue;
            }
        }
        if due_runs.is_empty() {
            let mut updated = automation;
            if scheduled > now {
                continue;
            }
            if scheduled <= now && matches!(updated.misfire, crate::core::MisfirePolicy::Skip) {
                Registry::open(&registry_path)?.append_event(&Event {
                    run_id: None,
                    occurred_at: now,
                    event_type: "scheduler.misfire_skipped".into(),
                    payload: serde_json::json!({
                        "automation_id": updated.id,
                        "scheduled_at": scheduled.to_rfc3339(),
                        "misfire": "skip",
                    }),
                })?;
            }
            updated.next_run_at = scheduler::next_run(&updated.trigger, now)?;
            Registry::open(&registry_path)?.save_automation(&updated)?;
            continue;
        }
        let mut last_scheduled = None;
        for scheduled_at in due_runs {
            pass.due += 1;
            last_scheduled = Some(scheduled_at);
            match execute_automation_at(&registry_path, &automation, Some(scheduled_at)).await {
                Ok(result) if result.status == "succeeded" => {}
                Ok(_) | Err(_) => pass.failed += 1,
            }
        }
        let mut updated = automation;
        updated.next_run_at = match updated.misfire {
            crate::core::MisfirePolicy::Skip | crate::core::MisfirePolicy::RunOnce => {
                scheduler::next_run(&updated.trigger, now)?
            }
            crate::core::MisfirePolicy::CatchUp { .. } => {
                let after = last_scheduled.unwrap_or(now);
                scheduler::next_run(&updated.trigger, after)?
            }
        };
        Registry::open(&registry_path)?.save_automation(&updated)?;
    }
    Ok(pass)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::core::{CommandSpec, StepSpec};
    use crate::integrations::{IntegrationAction, MoleIntegration};
    use chrono::Duration;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[tokio::test]
    async fn shared_service_records_run_without_cli_output() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let automation = Automation {
            id: "service-test".into(),
            name: "service-test".into(),
            ownership: Ownership::Managed,
            steps: vec![StepSpec {
                id: "echo".into(),
                command: CommandSpec::argv("/bin/echo", ["rpc-ok"]),
                responses: None,
                integration: None,
            }],
            ..Automation::default()
        };
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();
        let result = run_named(&path, "service-test", false).await.unwrap();
        assert_eq!(result.status, "succeeded");
        assert_eq!(result.stdout.trim(), "rpc-ok");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mole_dry_run_uses_shared_run_event_metric_and_log_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let integration = MoleIntegration::new("/bin/echo", 2);
        let action =
            IntegrationAction::with_parameters("clean", serde_json::json!({"dry_run": true}))
                .unwrap();
        let execution = execute_integration(&path, &integration, &action)
            .await
            .unwrap();
        assert_eq!(execution.run.status, "succeeded");
        assert_eq!(execution.result.status, IntegrationStatus::Succeeded);
        assert_eq!(execution.verification.status, VerificationStatus::Passed);
        let registry = Registry::open(&path).unwrap();
        assert!(
            registry
                .list_events(50)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "integration.plan.created")
        );
        let logs = registry
            .get_run_logs(&execution.run.run_id)
            .unwrap()
            .unwrap();
        assert!(logs.stdout.contains("clean --dry-run"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scheduled_read_only_integration_is_typed_and_redacts_scanner_logs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let integration = crate::integrations::TopgradeIntegration::new("/bin/echo", 2);
        let action = IntegrationAction::with_parameters("plan", serde_json::json!({})).unwrap();
        let automation = create_integration_automation(
            &path,
            &integration,
            &action,
            "topgrade-plan".into(),
            None,
            Trigger::Manual,
        )
        .unwrap();
        assert!(
            automation.steps[0]
                .command
                .executable
                .as_os_str()
                .is_empty()
        );
        assert_eq!(
            automation.steps[0]
                .integration
                .as_ref()
                .map(|spec| spec.integration.as_str()),
            Some("topgrade")
        );
        let run = run_named(&path, "topgrade-plan", false).await.unwrap();
        assert_eq!(run.status, "succeeded");
        let stored = Registry::open(&path)
            .unwrap()
            .get_run_logs(&run.run_id)
            .unwrap()
            .unwrap();
        assert!(stored.stdout.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scheduled_write_integration_is_refused_before_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let integration = MoleIntegration::new("/bin/echo", 2);
        let action =
            IntegrationAction::with_parameters("clean", serde_json::json!({"dry_run": false}))
                .unwrap();
        let error = create_integration_automation(
            &path,
            &integration,
            &action,
            "mole-clean".into(),
            None,
            Trigger::Interval { seconds: 3600 },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("must be read-only or dry-run"));
        assert!(
            Registry::open(&path)
                .unwrap()
                .get_automation("mole-clean")
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mole_destructive_action_is_held_and_audited_before_spawn() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let integration = MoleIntegration::new("/bin/echo", 2);
        let action =
            IntegrationAction::with_parameters("clean", serde_json::json!({"dry_run": false}))
                .unwrap();
        let error = execute_integration(&path, &integration, &action)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("held for approval"));
        let registry = Registry::open(&path).unwrap();
        assert!(
            registry
                .list_events(20)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "integration.approval_required")
        );
        assert!(registry.list_runs(20, None).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn approved_integration_is_bound_to_plan_and_consumed_once() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let integration = MoleIntegration::new("/bin/echo", 2);
        let action =
            IntegrationAction::with_parameters("clean", serde_json::json!({"dry_run": false}))
                .unwrap();
        let approval = request_integration_approval(&path, &integration, &action, 3600).unwrap();
        Registry::open(&path)
            .unwrap()
            .decide_approval(&approval.id, "approved")
            .unwrap();
        let execution = execute_integration(
            &path,
            &integration,
            &action.clone().with_approval(&approval.id),
        )
        .await
        .unwrap();
        assert_eq!(execution.run.status, "succeeded");
        let stored = Registry::open(&path)
            .unwrap()
            .get_approval(&approval.id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, "consumed");
        let error = execute_integration(&path, &integration, &action.with_approval(&approval.id))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid, expired, already consumed"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn retry_policy_records_bounded_attempts_for_a_failed_step() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let mut automation = Automation {
            id: "retry-test".into(),
            name: "retry-test".into(),
            ownership: Ownership::Managed,
            steps: vec![StepSpec {
                id: "false".into(),
                command: CommandSpec::argv("/bin/false", Vec::<String>::new()),
                responses: None,
                integration: None,
            }],
            ..Automation::default()
        };
        automation.retry.max_attempts = 3;
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();
        let result = execute_automation(&path, &automation).await.unwrap();
        assert_eq!(result.status, "failed");
        let retry_events = Registry::open(&path)
            .unwrap()
            .list_events(20)
            .unwrap()
            .into_iter()
            .filter(|event| event.event_type == "executor.command.retrying")
            .count();
        assert_eq!(retry_events, 2);
        let failed_events = Registry::open(&path)
            .unwrap()
            .list_events(20)
            .unwrap()
            .into_iter()
            .filter(|event| event.event_type == "executor.command.failed")
            .count();
        assert_eq!(failed_events, 3);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn manual_run_respects_forbid_overlap_admission() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let automation = Automation {
            id: "overlap-test".into(),
            name: "overlap-test".into(),
            ownership: Ownership::Managed,
            steps: vec![StepSpec {
                id: "sleep".into(),
                command: CommandSpec::argv("/bin/sleep", ["10"]),
                responses: None,
                integration: None,
            }],
            ..Automation::default()
        };
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();
        let first_path = path.clone();
        let first_automation = automation.clone();
        let first =
            tokio::spawn(async move { execute_automation(&first_path, &first_automation).await });
        let first_run_id = loop {
            if let Some(run) = Registry::open(&path)
                .unwrap()
                .list_runs(1, Some("overlap-test"))
                .unwrap()
                .into_iter()
                .find(|run| run.status == "running")
            {
                break run.id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        };
        let second = execute_automation(&path, &automation).await.unwrap_err();
        assert!(second.to_string().contains("forbids overlap"));
        assert!(
            Registry::open(&path)
                .unwrap()
                .list_events(20)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "run.admission_rejected")
        );
        cancel_run(&path, &first_run_id).unwrap();
        assert_eq!(first.await.unwrap().unwrap().status, "cancelled");
        assert_eq!(
            Registry::open(&path)
                .unwrap()
                .list_runs(10, Some("overlap-test"))
                .unwrap()
                .len(),
            1
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scheduled_pass_applies_run_once_misfire_and_persists_schedule() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let scheduled_at = Utc::now() - Duration::minutes(3);
        let automation = Automation {
            id: "scheduled-test".into(),
            name: "scheduled-test".into(),
            ownership: Ownership::Managed,
            trigger: Trigger::Interval { seconds: 60 },
            misfire: crate::core::MisfirePolicy::RunOnce,
            next_run_at: Some(scheduled_at),
            steps: vec![StepSpec {
                id: "echo".into(),
                command: CommandSpec::argv("/bin/echo", ["scheduled"]),
                responses: None,
                integration: None,
            }],
            ..Automation::default()
        };
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();

        let pass = scheduled_pass(&path).await.unwrap();
        assert_eq!(pass.due, 1);
        assert_eq!(pass.failed, 0);
        let registry = Registry::open(&path).unwrap();
        let runs = registry.list_runs(10, Some("scheduled-test")).unwrap();
        assert_eq!(runs.len(), 1);
        let scheduled_text = scheduled_at.to_rfc3339();
        assert_eq!(
            runs[0].scheduled_at.as_deref(),
            Some(scheduled_text.as_str())
        );
        assert!(
            registry
                .get_automation("scheduled-test")
                .unwrap()
                .unwrap()
                .next_run_at
                .unwrap()
                > Utc::now()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scheduled_pass_audits_skipped_misfire_without_creating_a_run() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let scheduled_at = Utc::now() - Duration::minutes(3);
        let automation = Automation {
            id: "skip-misfire-test".into(),
            name: "skip-misfire-test".into(),
            ownership: Ownership::Managed,
            trigger: crate::core::Trigger::Interval { seconds: 60 },
            misfire: crate::core::MisfirePolicy::Skip,
            next_run_at: Some(scheduled_at),
            steps: vec![StepSpec {
                id: "should-not-run".into(),
                command: CommandSpec::argv("/bin/false", Vec::<String>::new()),
                responses: None,
                integration: None,
            }],
            ..Automation::default()
        };
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();

        let pass = scheduled_pass(&path).await.unwrap();
        assert_eq!(pass.due, 0);
        assert_eq!(pass.failed, 0);
        let registry = Registry::open(&path).unwrap();
        assert!(
            registry
                .list_runs(10, Some("skip-misfire-test"))
                .unwrap()
                .is_empty()
        );
        let events = registry.list_events(10).unwrap();
        assert!(events.iter().any(|event| {
            event.event_type == "scheduler.misfire_skipped"
                && event.payload["misfire"] == "skip"
                && event.payload["scheduled_at"] == scheduled_at.to_rfc3339()
        }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scheduled_pass_preserves_a_future_next_run() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let next_run_at = Utc::now() + Duration::hours(1);
        let automation = Automation {
            id: "future-schedule-test".into(),
            name: "future-schedule-test".into(),
            ownership: Ownership::Managed,
            trigger: crate::core::Trigger::Interval { seconds: 60 },
            next_run_at: Some(next_run_at),
            steps: vec![StepSpec {
                id: "should-not-run".into(),
                command: CommandSpec::argv("/bin/false", Vec::<String>::new()),
                responses: None,
                integration: None,
            }],
            ..Automation::default()
        };
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();

        let pass = scheduled_pass(&path).await.unwrap();
        assert_eq!(pass.due, 0);
        assert_eq!(pass.failed, 0);
        let stored = Registry::open(&path)
            .unwrap()
            .get_automation("future-schedule-test")
            .unwrap()
            .unwrap();
        assert_eq!(stored.next_run_at, Some(next_run_at));
        assert!(
            Registry::open(&path)
                .unwrap()
                .list_events(10)
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scheduled_pass_expires_an_overage_misfire_without_running_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let scheduled_at = Utc::now() - Duration::hours(2);
        let automation = Automation {
            id: "expired-misfire-test".into(),
            name: "expired-misfire-test".into(),
            ownership: Ownership::Managed,
            trigger: crate::core::Trigger::Interval { seconds: 60 },
            misfire: crate::core::MisfirePolicy::RunOnce,
            misfire_max_age_seconds: Some(60),
            next_run_at: Some(scheduled_at),
            steps: vec![StepSpec {
                id: "should-not-run".into(),
                command: CommandSpec::argv("/bin/false", Vec::<String>::new()),
                responses: None,
                integration: None,
            }],
            ..Automation::default()
        };
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();

        let pass = scheduled_pass(&path).await.unwrap();
        assert_eq!(pass.due, 0);
        assert_eq!(pass.failed, 0);
        let registry = Registry::open(&path).unwrap();
        assert!(
            registry
                .list_runs(10, Some("expired-misfire-test"))
                .unwrap()
                .is_empty()
        );
        assert!(registry.list_events(10).unwrap().iter().any(|event| {
            event.event_type == "scheduler.misfire_expired"
                && event.payload["max_age_seconds"] == 60
        }));
        assert!(
            registry
                .get_automation("expired-misfire-test")
                .unwrap()
                .unwrap()
                .next_run_at
                .unwrap()
                > Utc::now()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_run_signals_the_active_executor_and_records_cancelled() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("registry.sqlite3");
        let automation = Automation {
            id: "cancel-test".into(),
            name: "cancel-test".into(),
            ownership: Ownership::Managed,
            steps: vec![StepSpec {
                id: "sleep".into(),
                command: CommandSpec::argv("/bin/sleep", ["10"]),
                responses: None,
                integration: None,
            }],
            ..Automation::default()
        };
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();
        let task_path = path.clone();
        let task_automation = automation.clone();
        let task =
            tokio::spawn(async move { execute_automation(&task_path, &task_automation).await });
        let run_id = loop {
            if let Some(run) = Registry::open(&path)
                .unwrap()
                .list_runs(1, Some("cancel-test"))
                .unwrap()
                .into_iter()
                .find(|run| run.status == "running")
            {
                break run.id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        };
        cancel_run(&path, &run_id).unwrap();
        let result = task.await.unwrap().unwrap();
        assert_eq!(result.status, "cancelled");
        let registry = Registry::open(&path).unwrap();
        assert!(
            registry
                .list_events(20)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "run.cancel_requested")
        );
        assert!(
            registry
                .list_events(20)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "run.cancelled")
        );
        assert!(
            registry
                .list_events(20)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "executor.command.cancelled")
        );
    }
}
