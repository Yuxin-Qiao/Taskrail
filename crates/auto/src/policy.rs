use crate::core::Automation;
use chrono::Utc;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PolicyIssue {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyReport {
    pub automation_id: String,
    pub revision: u64,
    pub status: String,
    pub issues: Vec<PolicyIssue>,
}

pub fn check(automation: &Automation) -> PolicyReport {
    let mut issues = Vec::new();
    if automation.steps.is_empty() {
        issue(
            &mut issues,
            "error",
            "no_steps",
            "automation must contain at least one step",
        );
    }
    if automation.steps.len() > automation.policy.budget.max_steps as usize {
        issue(
            &mut issues,
            "error",
            "max_steps_exceeded",
            format!(
                "{} steps exceed budget.max_steps={}",
                automation.steps.len(),
                automation.policy.budget.max_steps
            ),
        );
    }
    if automation.policy.wall_time_seconds == 0 {
        issue(
            &mut issues,
            "error",
            "zero_timeout",
            "policy wall_time_seconds must be greater than zero",
        );
    }
    if automation.policy.retry.max_attempts == 0 {
        issue(
            &mut issues,
            "error",
            "zero_attempts",
            "policy retry.max_attempts must be at least one",
        );
    }
    if automation.policy.retry.initial_backoff_seconds > automation.policy.retry.max_backoff_seconds
    {
        issue(
            &mut issues,
            "error",
            "backoff_exceeds_max",
            "policy retry.initial_backoff_seconds exceeds max_backoff_seconds",
        );
    }
    if matches!(
        automation.misfire,
        crate::core::MisfirePolicy::CatchUp { max_runs: 0 }
    ) {
        issue(
            &mut issues,
            "error",
            "zero_catch_up_runs",
            "misfire catch_up.max_runs must be at least one",
        );
    }
    if let Err(error) = crate::scheduler::next_run(&automation.trigger, Utc::now()) {
        issue(
            &mut issues,
            "error",
            "invalid_trigger",
            format!("trigger preflight failed: {error}"),
        );
    }
    if automation.runtime_state == crate::core::RuntimeState::NeedsAttention {
        issue(
            &mut issues,
            "error",
            "needs_attention",
            "automation requires explicit drift or recovery handling",
        );
    }
    if automation.ownership == crate::core::Ownership::Observed {
        issue(
            &mut issues,
            "warning",
            "observed_only",
            "native scheduler remains authoritative; execution requires an explicit observed override",
        );
    }
    for step in &automation.steps {
        let has_command = !step.command.executable.as_os_str().is_empty();
        let has_responses = step.responses.is_some();
        if !has_command && !has_responses {
            issue(
                &mut issues,
                "error",
                "missing_executor",
                format!(
                    "step {} has neither a command nor a Responses API request",
                    step.id
                ),
            );
        }
        if has_command && has_responses {
            issue(
                &mut issues,
                "error",
                "multiple_executors",
                format!(
                    "step {} configures both a command and a Responses API request",
                    step.id
                ),
            );
        }
        if has_command && (step.command.shell || step.command.invokes_shell()) {
            issue(
                &mut issues,
                "error",
                "shell_execution",
                format!("step {} requests shell execution", step.id),
            );
        }
        if let Some(responses) = &step.responses {
            if responses.prompt.trim().is_empty() {
                issue(
                    &mut issues,
                    "error",
                    "missing_prompt",
                    format!("step {} has an empty Responses API prompt", step.id),
                );
            }
            if responses.api_key_env.trim().is_empty() {
                issue(
                    &mut issues,
                    "error",
                    "missing_api_key_env",
                    format!(
                        "step {} has an empty API key environment variable name",
                        step.id
                    ),
                );
            }
        }
        if step.risk > automation.policy.max_risk {
            issue(
                &mut issues,
                "error",
                "risk_exceeds_policy",
                format!(
                    "step {} risk {} exceeds policy max {}",
                    step.id,
                    step.risk.label(),
                    automation.policy.max_risk.label()
                ),
            );
        }
        if step.risk.requires_approval() || automation.policy.approval_required {
            issue(
                &mut issues,
                "error",
                "approval_required",
                format!("step {} requires an approval-aware execution path", step.id),
            );
        }
    }
    let status = if issues.iter().any(|issue| issue.severity == "error") {
        "fail"
    } else if issues.iter().any(|issue| issue.severity == "warning") {
        "warn"
    } else {
        "pass"
    };
    PolicyReport {
        automation_id: automation.id.clone(),
        revision: automation.revision,
        status: status.into(),
        issues,
    }
}

fn issue(issues: &mut Vec<PolicyIssue>, severity: &str, code: &str, message: impl Into<String>) {
    issues.push(PolicyIssue {
        severity: severity.into(),
        code: code.into(),
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CommandSpec, Ownership, ResponsesSpec, Risk, StepSpec};

    #[test]
    fn reports_a_clean_managed_read_only_automation() {
        let automation = Automation {
            id: "policy-ok".into(),
            name: "policy-ok".into(),
            ownership: Ownership::Managed,
            steps: vec![StepSpec {
                id: "echo".into(),
                command: CommandSpec::argv("/bin/echo", ["ok"]),
                responses: None,
                risk: Risk::R0Read,
            }],
            ..Automation::default()
        };
        let report = check(&automation);
        assert_eq!(report.status, "pass");
        assert!(report.issues.is_empty());
    }

    #[test]
    fn accepts_a_read_only_responses_step_without_a_command() {
        let automation = Automation {
            id: "responses-ok".into(),
            name: "responses-ok".into(),
            ownership: Ownership::Managed,
            steps: vec![StepSpec {
                id: "summarize".into(),
                command: CommandSpec::default(),
                responses: Some(ResponsesSpec {
                    prompt: "Summarize this.".into(),
                    base_url: Some("https://example.test/v1".into()),
                    model: Some("test-model".into()),
                    api_key_env: "TEST_API_KEY".into(),
                    store: false,
                }),
                risk: Risk::R0Read,
            }],
            ..Automation::default()
        };
        let report = check(&automation);
        assert_eq!(report.status, "pass");
        assert!(report.issues.is_empty());
    }

    #[test]
    fn reports_all_relevant_fail_closed_policy_violations() {
        let mut automation = Automation {
            id: "policy-bad".into(),
            ..Automation::default()
        };
        automation.policy.budget.max_steps = 0;
        automation.policy.wall_time_seconds = 0;
        automation.policy.retry.max_attempts = 0;
        automation.policy.retry.initial_backoff_seconds = 10;
        automation.policy.retry.max_backoff_seconds = 1;
        automation.misfire = crate::core::MisfirePolicy::CatchUp { max_runs: 0 };
        automation.trigger = crate::core::Trigger::Interval { seconds: 0 };
        automation.steps = vec![StepSpec {
            id: "unsafe".into(),
            command: CommandSpec::argv("/bin/sh", ["-c", "echo unsafe"]),
            responses: None,
            risk: Risk::R2ExternalWrite,
        }];
        let report = check(&automation);
        assert_eq!(report.status, "fail");
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "max_steps_exceeded")
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "shell_execution")
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "approval_required")
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "zero_attempts")
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "backoff_exceeds_max")
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "zero_catch_up_runs")
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "invalid_trigger")
        );
    }
}
