use super::{
    helpers::{
        collect_error_findings, failed_result, first_line, json_lines, metric, numeric,
        resolve_executable, result, safe_text,
    },
    model::{
        Capability, Change, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult,
        DoctorStatus, EnvironmentRef, ExecutionPlan, Finding, IntegrationAction,
        IntegrationDescriptor, IntegrationId, IntegrationLevel, IntegrationResult,
        IntegrationStatus, ProcessOutput, ProcessStatus, RiskClass, VerificationCheck,
        VerificationResult, VerificationStatus,
    },
    registry::Integration,
};
use crate::core::CommandSpec;
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, process::Command};

const INTEGRATION_ID: &str = "rclone";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;

#[derive(Debug, Clone)]
pub struct RcloneIntegration {
    descriptor: IntegrationDescriptor,
    executable: PathBuf,
    timeout_seconds: u64,
}

impl Default for RcloneIntegration {
    fn default() -> Self {
        Self::new("rclone", DEFAULT_TIMEOUT_SECONDS)
    }
}

impl RcloneIntegration {
    pub fn new(executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(INTEGRATION_ID).expect("static rclone integration id"),
                display_name: "rclone".into(),
                level: IntegrationLevel::Semantic,
                capabilities: vec![
                    Capability::new("doctor", RiskClass::Read, false),
                    Capability::new("list-remotes", RiskClass::Read, false),
                    Capability::new("check", RiskClass::Read, false),
                    Capability::new("copy", RiskClass::NetworkWrite, false),
                    Capability::new("sync", RiskClass::Destructive, true),
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
            "doctor" => vec!["version".into()],
            "list-remotes" => vec!["listremotes".into()],
            "check" => {
                let (source, destination) = source_destination(action)?;
                vec!["check".into(), source, destination, "--json".into()]
            }
            "copy" => {
                let (source, destination) = source_destination(action)?;
                vec![
                    "copy".into(),
                    source,
                    destination,
                    "--stats-one-line".into(),
                    "--json".into(),
                ]
            }
            "sync" => {
                let (source, destination) = source_destination(action)?;
                let mut args = vec![
                    "sync".into(),
                    source,
                    destination,
                    "--stats-one-line".into(),
                    "--json".into(),
                ];
                if action_is_dry_run(action) {
                    args.push("--dry-run".into());
                }
                args
            }
            unsupported => anyhow::bail!("unsupported rclone action: {unsupported}"),
        };
        Ok(CommandSpec::argv(self.executable.clone(), args))
    }

    fn environment_refs(&self, action: &IntegrationAction) -> Result<Vec<EnvironmentRef>> {
        let reference = action
            .parameters
            .get("config_env")
            .and_then(Value::as_str)
            .map(|name| EnvironmentRef::new(name, false))
            .transpose()?;
        Ok(reference.into_iter().collect())
    }
}

impl Integration for RcloneIntegration {
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
                detail: Some("rclone executable was not found on PATH".into()),
            };
        };
        match Command::new(&executable).arg("version").output() {
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
                detail: Some(format!("rclone version exited with {}", output.status)),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!("failed to execute rclone: {error}")),
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
                name: "rclone_cli".into(),
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
            "copy" => RiskClass::NetworkWrite,
            "sync" if dry_run => RiskClass::Read,
            "sync" => RiskClass::Destructive,
            "doctor" | "list-remotes" | "check" => RiskClass::Read,
            unsupported => anyhow::bail!("unsupported rclone action: {unsupported}"),
        };
        let plan = ExecutionPlan {
            integration: self.id().clone(),
            action: action.action.clone(),
            command: self.command(action)?,
            environment_refs: self.environment_refs(action)?,
            risk,
            requires_approval: risk.requires_approval(),
            supports_dry_run: action.action == "sync",
            dry_run,
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
            return Ok(failed_result(self.id(), &action.action, &output, "rclone"));
        }
        match action.action.as_str() {
            "doctor" => {
                let version = first_line(output.stdout.as_bytes())
                    .or_else(|| first_line(output.stderr.as_bytes()))
                    .unwrap_or_else(|| "rclone CLI responded successfully".into());
                Ok(result(
                    self.id(),
                    &action.action,
                    IntegrationStatus::Succeeded,
                    safe_text(&version),
                    BTreeMap::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ))
            }
            "list-remotes" => parse_remotes(self.id(), action, &output.stdout),
            "check" | "copy" | "sync" => parse_transfer(self.id(), action, &output.stdout),
            unsupported => anyhow::bail!("unsupported rclone action: {unsupported}"),
        }
    }

    fn verify(
        &self,
        action: &IntegrationAction,
        result: &IntegrationResult,
    ) -> Result<VerificationResult> {
        let failed = result.status == IntegrationStatus::Failed;
        let (status, detail) = match action.action.as_str() {
            "sync" if action_is_dry_run(action) && !failed => (
                VerificationStatus::Passed,
                "rclone sync dry-run produced a bounded transfer plan.",
            ),
            "sync" if !failed => (
                VerificationStatus::NotConfigured,
                "rclone sync can delete destination files; post-sync verification is not configured.",
            ),
            _ if !failed && result.findings.is_empty() => (
                VerificationStatus::Passed,
                "rclone command completed without normalized errors.",
            ),
            _ if !failed => (
                VerificationStatus::Failed,
                "rclone reported normalized errors.",
            ),
            _ => (
                VerificationStatus::Failed,
                "rclone command did not complete successfully.",
            ),
        };
        Ok(VerificationResult {
            status,
            checks: vec![VerificationCheck {
                name: "rclone_result".into(),
                status,
                detail: detail.into(),
            }],
        })
    }
}

fn action_is_dry_run(action: &IntegrationAction) -> bool {
    action.action == "sync"
        && action
            .parameters
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn source_destination(action: &IntegrationAction) -> Result<(String, String)> {
    let source = required_transfer_arg(action, "source")?;
    let destination = required_transfer_arg(action, "destination")?;
    Ok((source, destination))
}

fn required_transfer_arg(action: &IntegrationAction, key: &str) -> Result<String> {
    let value = action
        .parameters
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| {
            format!(
                "rclone {} requires a string parameter named {key}",
                action.action
            )
        })?;
    if value.trim().is_empty() || value.starts_with('-') || value.contains('\0') {
        anyhow::bail!("rclone {key} must be a non-empty non-option value");
    }
    Ok(value.into())
}

fn parse_remotes(
    integration: &IntegrationId,
    action: &IntegrationAction,
    stdout: &str,
) -> Result<IntegrationResult> {
    let mut changes = Vec::new();
    for remote in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(200)
    {
        changes.push(Change {
            kind: "remote".into(),
            description: safe_text(remote),
            count: Some(1),
        });
    }
    Ok(result(
        integration,
        &action.action,
        IntegrationStatus::Succeeded,
        format!("rclone found {} configured remote(s).", changes.len()),
        BTreeMap::new(),
        Vec::new(),
        changes,
        Vec::new(),
    ))
}

fn parse_transfer(
    integration: &IntegrationId,
    action: &IntegrationAction,
    stdout: &str,
) -> Result<IntegrationResult> {
    if stdout.trim().is_empty() {
        return Ok(result(
            integration,
            &action.action,
            IntegrationStatus::Succeeded,
            format!(
                "rclone {} completed with no reported transfers.",
                action.action
            ),
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ));
    }
    let values = json_lines(stdout)
        .with_context(|| format!("parse rclone {} structured output", action.action))?;
    let mut metrics = BTreeMap::new();
    let mut changes = Vec::new();
    let mut findings = collect_error_findings(&values, "rclone");
    for value in &values {
        let Some(object) = value.as_object() else {
            continue;
        };
        for key in [
            "bytes",
            "transfers",
            "checks",
            "errors",
            "deletes",
            "deletesSkipped",
        ] {
            if let Some(number) = object.get(key).and_then(numeric) {
                let unit = if key == "bytes" { "bytes" } else { "count" };
                let entry = metrics
                    .entry(key.into())
                    .or_insert_with(|| metric(0.0, unit));
                entry.value = entry.value.max(number);
                if key == "transfers" || key == "deletes" || key == "deletesSkipped" {
                    changes.push(Change {
                        kind: key.into(),
                        description: format!("rclone {key}"),
                        count: Some(number.max(0.0) as u64),
                    });
                }
                if key == "errors" && number > 0.0 {
                    findings.push(Finding {
                        kind: "error".into(),
                        title: format!("rclone reported {number:.0} error(s)"),
                        severity: Some("high".into()),
                        location: None,
                        fingerprint: None,
                    });
                }
            }
        }
        if let Some(message) = object.get("error").and_then(Value::as_str) {
            findings.push(Finding {
                kind: "error".into(),
                title: format!("rclone: {}", safe_text(message)),
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
    let summary = if action.action == "sync" && action_is_dry_run(action) {
        "rclone sync dry-run completed.".into()
    } else {
        format!("rclone {} completed.", action.action)
    };
    Ok(result(
        integration,
        &action.action,
        status,
        summary,
        metrics,
        findings,
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
            duration_ms: 10,
        }
    }

    #[test]
    fn plans_typed_transfer_argv_and_distinguishes_sync_dry_run() {
        let integration = RcloneIntegration::new("rclone", 60);
        let dry = IntegrationAction::with_parameters(
            "sync",
            serde_json::json!({"source":"/tmp/source","destination":"remote:backup","dry_run":true}),
        )
        .unwrap();
        let plan = integration.plan(&dry).unwrap();
        assert_eq!(plan.risk, RiskClass::Read);
        assert!(plan.command.args.contains(&"--dry-run".into()));
        assert!(!plan.requires_approval);
        let real = IntegrationAction::with_parameters(
            "sync",
            serde_json::json!({"source":"/tmp/source","destination":"remote:backup","dry_run":false}),
        )
        .unwrap();
        assert_eq!(
            integration.plan(&real).unwrap().risk,
            RiskClass::Destructive
        );
        assert!(integration.plan(&real).unwrap().requires_approval);
    }

    #[test]
    fn parses_remotes_and_transfer_metrics_without_secret_values() {
        let integration = RcloneIntegration::default();
        let remotes = IntegrationAction::new("list-remotes").unwrap();
        let result = integration
            .parse(
                &remotes,
                output(ProcessStatus::Succeeded, "backup:\nmedia:\n"),
            )
            .unwrap();
        assert_eq!(result.changes.len(), 2);
        let copy = IntegrationAction::with_parameters(
            "copy",
            serde_json::json!({"source":"/tmp/source","destination":"remote:backup"}),
        )
        .unwrap();
        let result = integration
            .parse(
                &copy,
                output(
                    ProcessStatus::Succeeded,
                    r#"{"bytes":2048,"transfers":3,"checks":4,"errors":0}"#,
                ),
            )
            .unwrap();
        assert_eq!(result.metrics["bytes"].unit, "bytes");
        assert_eq!(result.metrics["transfers"].value, 3.0);
        assert!(
            integration
                .parse(&copy, output(ProcessStatus::Succeeded, "not-json"))
                .is_err()
        );
        assert!(!serde_json::to_string(&result).unwrap().contains("password"));
    }

    #[test]
    fn handles_error_lines_and_missing_executable() {
        let integration = RcloneIntegration::default();
        let check = IntegrationAction::with_parameters(
            "check",
            serde_json::json!({"source":"remote:source","destination":"/tmp/dest"}),
        )
        .unwrap();
        let result = integration
            .parse(
                &check,
                output(
                    ProcessStatus::Succeeded,
                    r#"{"errors":1,"error":"remote unavailable"}"#,
                ),
            )
            .unwrap();
        assert_eq!(result.status, IntegrationStatus::NeedsAttention);
        assert_eq!(
            integration.verify(&check, &result).unwrap().status,
            VerificationStatus::Failed
        );
        let detection = RcloneIntegration::new("/definitely/missing/rclone", 60).detect();
        assert_eq!(detection.status, DetectionStatus::Missing);
    }
}
