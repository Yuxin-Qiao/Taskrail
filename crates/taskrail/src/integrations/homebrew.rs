use super::{
    helpers::{
        failed_result, first_line, json_lines, metric, resolve_executable, result, safe_text,
    },
    model::{
        Capability, Change, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult,
        DoctorStatus, ExecutionPlan, IntegrationAction, IntegrationDescriptor, IntegrationId,
        IntegrationLevel, IntegrationResult, IntegrationStatus, ProcessOutput, ProcessStatus,
        RiskClass, VerificationCheck, VerificationResult, VerificationStatus,
    },
    registry::Integration,
};
use crate::core::CommandSpec;
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, process::Command};

const INTEGRATION_ID: &str = "homebrew";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;

#[derive(Debug, Clone)]
pub struct HomebrewIntegration {
    descriptor: IntegrationDescriptor,
    executable: PathBuf,
    timeout_seconds: u64,
}

impl Default for HomebrewIntegration {
    fn default() -> Self {
        Self::new("brew", DEFAULT_TIMEOUT_SECONDS)
    }
}

impl HomebrewIntegration {
    pub fn new(executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(INTEGRATION_ID).expect("static Homebrew integration id"),
                display_name: "Homebrew".into(),
                level: IntegrationLevel::Semantic,
                capabilities: vec![
                    Capability::new("doctor", RiskClass::Read, false),
                    Capability::new("outdated", RiskClass::Read, false),
                    Capability::new("bundle-check", RiskClass::Read, false),
                    Capability::new("upgrade", RiskClass::SystemWrite, true),
                    Capability::new("cleanup", RiskClass::SystemWrite, true),
                ],
            },
            executable: executable.into(),
            timeout_seconds,
        }
    }

    pub fn id(&self) -> &IntegrationId {
        &self.descriptor.id
    }

    fn command(&self, action: &IntegrationAction) -> Result<CommandSpec> {
        let args = match action.action.as_str() {
            "doctor" => vec!["--version".into()],
            "outdated" => vec!["outdated".into(), "--json=v2".into()],
            "bundle-check" => vec![
                "bundle".into(),
                "check".into(),
                "--file".into(),
                required_file(action)?,
            ],
            "upgrade" | "cleanup" => {
                let mut args = vec![action.action.clone()];
                if action_is_dry_run(action) {
                    args.push("--dry-run".into());
                }
                args
            }
            unsupported => anyhow::bail!("unsupported Homebrew action: {unsupported}"),
        };
        Ok(CommandSpec::argv(self.executable.clone(), args))
    }
}

impl Integration for HomebrewIntegration {
    fn descriptor(&self) -> &IntegrationDescriptor {
        &self.descriptor
    }

    fn detect(&self) -> DetectionResult {
        let Some(executable) = resolve_executable(&self.executable) else {
            return DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Missing,
                executable: None,
                version: None,
                detail: Some("Homebrew executable was not found on PATH".into()),
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
                detail: Some(format!("brew --version exited with {}", output.status)),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!("failed to execute brew: {error}")),
            },
        }
    }

    fn doctor(&self) -> DoctorResult {
        let detection = self.detect();
        let ready = detection.status == DetectionStatus::Available;
        DoctorResult {
            integration: self.id().clone(),
            status: match detection.status {
                DetectionStatus::Available => DoctorStatus::Ready,
                DetectionStatus::Missing => DoctorStatus::Unavailable,
                DetectionStatus::Broken => DoctorStatus::NeedsConfiguration,
            },
            checks: vec![DoctorCheck {
                name: "brew_cli".into(),
                ok: ready,
                detail: detection
                    .version
                    .unwrap_or_else(|| detection.detail.unwrap_or_default()),
            }],
        }
    }

    fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan> {
        let dry_run = action_is_dry_run(action);
        let risk = match action.action.as_str() {
            "upgrade" | "cleanup" if !dry_run => RiskClass::SystemWrite,
            "doctor" | "outdated" | "bundle-check" | "upgrade" | "cleanup" => RiskClass::Read,
            unsupported => anyhow::bail!("unsupported Homebrew action: {unsupported}"),
        };
        let plan = ExecutionPlan {
            integration: self.id().clone(),
            action: action.action.clone(),
            command: self.command(action)?,
            environment_refs: Vec::new(),
            risk,
            requires_approval: risk.requires_approval(),
            supports_dry_run: matches!(action.action.as_str(), "upgrade" | "cleanup"),
            dry_run,
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
                "Homebrew",
            ));
        }
        if action.action == "doctor" {
            let line = first_line(output.stdout.as_bytes())
                .unwrap_or_else(|| "Homebrew CLI responded successfully".into());
            return Ok(result(
                self.id(),
                &action.action,
                IntegrationStatus::Succeeded,
                safe_text(&line),
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ));
        }
        if action.action == "bundle-check" && output.stdout.trim().is_empty() {
            return Ok(result(
                self.id(),
                &action.action,
                IntegrationStatus::Succeeded,
                "Homebrew bundle check completed.",
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ));
        }
        if action.action == "outdated" {
            return parse_outdated(self.id(), action, &output.stdout);
        }
        Ok(result(
            self.id(),
            &action.action,
            IntegrationStatus::Succeeded,
            if action_is_dry_run(action) {
                format!("Homebrew {} dry-run completed.", action.action)
            } else {
                format!("Homebrew {} completed.", action.action)
            },
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ))
    }

    fn verify(
        &self,
        action: &IntegrationAction,
        result: &IntegrationResult,
    ) -> Result<VerificationResult> {
        let status = if result.status == IntegrationStatus::Failed {
            VerificationStatus::Failed
        } else if matches!(action.action.as_str(), "upgrade" | "cleanup")
            && !action_is_dry_run(action)
        {
            VerificationStatus::NotConfigured
        } else {
            VerificationStatus::Passed
        };
        Ok(VerificationResult {
            status,
            checks: vec![VerificationCheck {
                name: "homebrew_result".into(),
                status,
                detail: if status == VerificationStatus::NotConfigured {
                    "Homebrew write action completed without a deterministic post-state check."
                        .into()
                } else {
                    "Homebrew command completed through the typed integration path.".into()
                },
            }],
        })
    }
}

fn action_is_dry_run(action: &IntegrationAction) -> bool {
    matches!(action.action.as_str(), "upgrade" | "cleanup")
        && action
            .parameters
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn required_file(action: &IntegrationAction) -> Result<String> {
    let file = action
        .parameters
        .get("file")
        .and_then(Value::as_str)
        .context("Homebrew bundle-check requires a file parameter")?;
    if file.trim().is_empty() || file.starts_with('-') || file.contains('\0') {
        anyhow::bail!("Homebrew bundle file must be a non-empty non-option value");
    }
    Ok(file.into())
}

fn parse_outdated(
    integration: &IntegrationId,
    action: &IntegrationAction,
    stdout: &str,
) -> Result<IntegrationResult> {
    let values = json_lines(stdout).with_context(|| "parse Homebrew outdated JSON output")?;
    let mut changes = Vec::new();
    for value in &values {
        for key in ["formulae", "casks"] {
            if let Some(items) = value.get(key).and_then(Value::as_array) {
                for item in items.iter().take(200) {
                    let name = item
                        .get("name")
                        .or_else(|| item.get("current_version"))
                        .and_then(Value::as_str)
                        .map(safe_text)
                        .unwrap_or_else(|| key.into());
                    changes.push(Change {
                        kind: key.trim_end_matches('e').into(),
                        description: name,
                        count: Some(1),
                    });
                }
            }
        }
    }
    let mut metrics = BTreeMap::new();
    metrics.insert(
        "outdated_count".into(),
        metric(changes.len() as f64, "count"),
    );
    Ok(result(
        integration,
        &action.action,
        IntegrationStatus::Succeeded,
        format!("Homebrew found {} outdated package(s).", changes.len()),
        metrics,
        Vec::new(),
        changes,
        Vec::new(),
    ))
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
    fn holds_homebrew_writes_without_sudo_and_keeps_outdated_read_only() {
        let integration = HomebrewIntegration::new("brew", 60);
        let outdated = IntegrationAction::new("outdated").unwrap();
        assert_eq!(integration.plan(&outdated).unwrap().risk, RiskClass::Read);
        let upgrade =
            IntegrationAction::with_parameters("upgrade", serde_json::json!({"dry_run":false}))
                .unwrap();
        let plan = integration.plan(&upgrade).unwrap();
        assert_eq!(plan.risk, RiskClass::SystemWrite);
        assert!(plan.requires_approval);
        assert!(!plan.command.args.iter().any(|arg| arg == "sudo"));
    }

    #[test]
    fn normalizes_outdated_and_dry_run_results() {
        let integration = HomebrewIntegration::default();
        let action = IntegrationAction::new("outdated").unwrap();
        let result = integration
            .parse(
                &action,
                output(
                    ProcessStatus::Succeeded,
                    r#"{"formulae":[{"name":"openssl","current_version":"1"}],"casks":[]}"#,
                ),
            )
            .unwrap();
        assert_eq!(result.metrics["outdated_count"].value, 1.0);
        let cleanup =
            IntegrationAction::with_parameters("cleanup", serde_json::json!({"dry_run":true}))
                .unwrap();
        assert!(
            integration
                .parse(
                    &cleanup,
                    output(ProcessStatus::Succeeded, "Would remove files")
                )
                .unwrap()
                .summary
                .contains("dry-run")
        );
        let failed = integration
            .parse(&action, output(ProcessStatus::Failed, "password=secret"))
            .unwrap();
        assert!(!serde_json::to_string(&failed).unwrap().contains("secret"));
    }

    #[test]
    fn missing_brew_is_detected() {
        assert_eq!(
            HomebrewIntegration::new("/definitely/missing/brew", 60)
                .detect()
                .status,
            DetectionStatus::Missing
        );
    }
}
