use super::{
    helpers::{
        collect_error_findings, failed_result, first_line, json_lines, metric, result, safe_text,
    },
    model::{
        ArtifactRef, Capability, Change, DetectionResult, DetectionStatus, DoctorCheck,
        DoctorResult, DoctorStatus, ExecutionPlan, Finding, IntegrationAction,
        IntegrationDescriptor, IntegrationId, IntegrationLevel, IntegrationResult,
        IntegrationStatus, ProcessOutput, ProcessStatus, RiskClass, VerificationCheck,
        VerificationResult, VerificationStatus,
    },
    registry::Integration,
};
use crate::{
    core::CommandSpec,
    github::{GhQuery, QueryKind},
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, process::Command};

const INTEGRATION_ID: &str = "github";
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;

#[derive(Debug, Clone)]
pub struct GithubIntegration {
    descriptor: IntegrationDescriptor,
    executable: PathBuf,
    hostname: String,
    timeout_seconds: u64,
}

impl Default for GithubIntegration {
    fn default() -> Self {
        Self::new("gh", "github.com", DEFAULT_TIMEOUT_SECONDS)
    }
}

impl GithubIntegration {
    pub fn new(
        executable: impl Into<PathBuf>,
        hostname: impl Into<String>,
        timeout_seconds: u64,
    ) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(INTEGRATION_ID).expect("static GitHub integration id"),
                display_name: "GitHub CLI".into(),
                level: IntegrationLevel::Semantic,
                capabilities: vec![
                    Capability::new("doctor", RiskClass::Read, false),
                    Capability::new("issues", RiskClass::Read, false),
                    Capability::new("pulls", RiskClass::Read, false),
                    Capability::new("failed-runs", RiskClass::Read, false),
                    Capability::new("checks", RiskClass::Read, false),
                ],
            },
            executable: executable.into(),
            hostname: hostname.into(),
            timeout_seconds,
        }
    }

    pub fn id(&self) -> &IntegrationId {
        &self.descriptor.id
    }

    fn query(&self, action: &IntegrationAction) -> Result<GhQuery> {
        let repo = action
            .parameters
            .get("repo")
            .and_then(Value::as_str)
            .context("GitHub action requires a repo owner/name parameter")?
            .to_owned();
        let kind = match action.action.as_str() {
            "issues" => QueryKind::Issues,
            "pulls" => QueryKind::Pulls,
            "failed-runs" => QueryKind::FailedRuns,
            "checks" => QueryKind::Checks,
            unsupported => anyhow::bail!("unsupported GitHub action: {unsupported}"),
        };
        Ok(GhQuery {
            repo,
            kind,
            pull_number: action.parameters.get("pull_number").and_then(Value::as_u64),
        })
    }

    fn command(&self, action: &IntegrationAction) -> Result<CommandSpec> {
        if action.action == "doctor" {
            validate_hostname(&self.hostname)?;
            return Ok(CommandSpec::argv(
                self.executable.clone(),
                ["auth", "status", "--hostname", &self.hostname],
            ));
        }
        self.query(action)?
            .command_spec_with_executable(self.executable.clone())
    }
}

impl Integration for GithubIntegration {
    fn descriptor(&self) -> &IntegrationDescriptor {
        &self.descriptor
    }

    fn detect(&self) -> DetectionResult {
        let Some(executable) = super::helpers::resolve_executable(&self.executable) else {
            return DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Missing,
                executable: None,
                version: None,
                detail: Some("GitHub CLI executable was not found on PATH".into()),
            };
        };
        match Command::new(&executable).arg("--version").output() {
            Ok(output) if output.status.success() => DetectionResult::available(
                self.id().clone(),
                executable,
                first_line(&output.stdout).or_else(|| first_line(&output.stderr)),
            ),
            Ok(output) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!("gh --version exited with {}", output.status)),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!("failed to execute gh: {error}")),
            },
        }
    }

    fn doctor(&self) -> DoctorResult {
        let detection = self.detect();
        if detection.status != DetectionStatus::Available {
            return DoctorResult {
                integration: self.id().clone(),
                status: DoctorStatus::Unavailable,
                checks: vec![DoctorCheck {
                    name: "gh_cli".into(),
                    ok: false,
                    detail: detection
                        .detail
                        .unwrap_or_else(|| "GitHub CLI unavailable".into()),
                }],
            };
        }
        let authenticated = Command::new(&self.executable)
            .args(["auth", "status", "--hostname", &self.hostname])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        DoctorResult {
            integration: self.id().clone(),
            status: if authenticated {
                DoctorStatus::Ready
            } else {
                DoctorStatus::NeedsConfiguration
            },
            checks: vec![
                DoctorCheck {
                    name: "gh_cli".into(),
                    ok: true,
                    detail: detection.version.unwrap_or_default(),
                },
                DoctorCheck {
                    name: "authentication".into(),
                    ok: authenticated,
                    detail: if authenticated {
                        format!("authenticated for {}", self.hostname)
                    } else {
                        format!("not authenticated for {}", self.hostname)
                    },
                },
            ],
        }
    }

    fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan> {
        let plan = ExecutionPlan {
            integration: self.id().clone(),
            action: action.action.clone(),
            command: self.command(action)?,
            environment_refs: Vec::new(),
            risk: RiskClass::Read,
            requires_approval: false,
            supports_dry_run: false,
            dry_run: false,
            plan_only: false,
            timeout_seconds: self.timeout_seconds,
            verification: None,
        };
        plan.validate()?;
        Ok(plan)
    }

    fn parse(
        &self,
        action: &IntegrationAction,
        output: ProcessOutput,
    ) -> Result<IntegrationResult> {
        if !matches!(output.status, ProcessStatus::Succeeded) {
            return Ok(failed_result(
                self.id(),
                &action.action,
                &output,
                "GitHub CLI",
            ));
        }
        if action.action == "doctor" {
            return Ok(result(
                self.id(),
                &action.action,
                IntegrationStatus::Succeeded,
                "GitHub CLI authentication check completed.",
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ));
        }
        let values = json_lines(&output.stdout)
            .with_context(|| format!("parse GitHub {} JSON output", action.action))?;
        let mut metrics = BTreeMap::new();
        metrics.insert("item_count".into(), metric(values.len() as f64, "count"));
        let mut changes = Vec::new();
        let mut artifacts = Vec::new();
        let mut findings = collect_error_findings(&values, "GitHub");
        for value in values.iter().take(200) {
            let Some(object) = value.as_object() else {
                continue;
            };
            let title = object
                .get("title")
                .or_else(|| object.get("name"))
                .and_then(Value::as_str)
                .map(safe_text)
                .unwrap_or_else(|| action.action.clone());
            let number = object
                .get("number")
                .and_then(Value::as_u64)
                .map(|number| number.to_string())
                .unwrap_or_else(|| "unknown".into());
            changes.push(Change {
                kind: action.action.clone(),
                description: format!("#{number} {title}"),
                count: Some(1),
            });
            if let Some(url) = object.get("url").and_then(Value::as_str) {
                artifacts.push(ArtifactRef {
                    kind: action.action.clone(),
                    reference: safe_text(url),
                });
            }
            if action.action == "checks"
                && object
                    .get("state")
                    .or_else(|| object.get("bucket"))
                    .and_then(Value::as_str)
                    .is_some_and(|state| {
                        matches!(
                            state.to_ascii_lowercase().as_str(),
                            "fail" | "failure" | "failed"
                        )
                    })
            {
                findings.push(Finding {
                    kind: "check_failure".into(),
                    title: format!("GitHub check failed: {title}"),
                    severity: Some("high".into()),
                    location: None,
                    fingerprint: None,
                });
            }
        }
        findings.truncate(100);
        let status = if findings.is_empty() {
            IntegrationStatus::Succeeded
        } else {
            IntegrationStatus::NeedsAttention
        };
        Ok(result(
            self.id(),
            &action.action,
            status,
            format!(
                "GitHub {} returned {} item(s).",
                action.action,
                values.len()
            ),
            metrics,
            findings,
            changes,
            artifacts,
        ))
    }

    fn verify(
        &self,
        _action: &IntegrationAction,
        result: &IntegrationResult,
    ) -> Result<VerificationResult> {
        let status = if result.status == IntegrationStatus::Failed {
            VerificationStatus::Failed
        } else {
            VerificationStatus::Passed
        };
        Ok(VerificationResult {
            status,
            checks: vec![VerificationCheck {
                name: "github_read_model".into(),
                status,
                detail: "GitHub read-only snapshot was normalized without write operations.".into(),
            }],
        })
    }
}

fn validate_hostname(hostname: &str) -> Result<()> {
    if hostname.trim().is_empty()
        || hostname.starts_with('-')
        || hostname.chars().any(char::is_whitespace)
        || hostname.len() > 253
    {
        anyhow::bail!("GitHub hostname must be a bounded host name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output(status: ProcessStatus, stdout: &str) -> ProcessOutput {
        ProcessOutput {
            status,
            exit_code: Some(if matches!(status, ProcessStatus::Succeeded) {
                0
            } else {
                1
            }),
            stdout: stdout.into(),
            stderr: String::new(),
            duration_ms: 5,
        }
    }

    #[test]
    fn reuses_fixed_read_only_gh_query_argv() {
        let integration = GithubIntegration::new("gh", "github.com", 60);
        let action =
            IntegrationAction::with_parameters("pulls", serde_json::json!({"repo":"owner/repo"}))
                .unwrap();
        let plan = integration.plan(&action).unwrap();
        assert_eq!(plan.risk, RiskClass::Read);
        assert!(plan.command.args.contains(&"--json".into()));
        assert!(
            !plan
                .command
                .args
                .iter()
                .any(|arg| arg == "api" || arg == "merge")
        );
    }

    #[test]
    fn normalizes_empty_and_failed_results_without_raw_output() {
        let integration = GithubIntegration::default();
        let action = IntegrationAction::with_parameters(
            "checks",
            serde_json::json!({"repo":"owner/repo","pull_number":3}),
        )
        .unwrap();
        let empty = integration
            .parse(&action, output(ProcessStatus::Succeeded, "[]"))
            .unwrap();
        assert_eq!(empty.metrics["item_count"].value, 0.0);
        let failed = integration
            .parse(&action, output(ProcessStatus::Failed, "token=do-not-echo"))
            .unwrap();
        assert_eq!(failed.status, IntegrationStatus::Failed);
        assert!(
            !serde_json::to_string(&failed)
                .unwrap()
                .contains("do-not-echo")
        );
    }

    #[test]
    fn missing_cli_is_detected_without_a_write_path() {
        assert_eq!(
            GithubIntegration::new("/definitely/missing/gh", "github.com", 60)
                .detect()
                .status,
            DetectionStatus::Missing
        );
    }
}
