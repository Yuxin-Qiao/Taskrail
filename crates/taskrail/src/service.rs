use crate::{
    core::{Automation, Event, Ownership, RunResult, RuntimeState, Trigger},
    executor, scheduler,
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
        if !has_command && !has_responses {
            anyhow::bail!("step {} has no configured executor", step.id);
        }
        if has_command && has_responses {
            anyhow::bail!("step {} configures multiple executors", step.id);
        }
        if let Some(spec) = &step.responses {
            if spec.prompt.trim().is_empty() {
                anyhow::bail!("step {} has an empty Responses API prompt", step.id);
            }
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
            let executor_kind = if step.responses.is_some() {
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
                        "argv": if executor_kind == "command" {
                            serde_json::Value::String(step.command.display())
                        } else {
                            serde_json::Value::Null
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CommandSpec, StepSpec};
    use chrono::Duration;
    use tempfile::tempdir;

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
