use super::{
    helpers::{
        collect_error_findings, failed_result, first_line, json_lines, metric, numeric,
        resolve_executable, result, safe_text, string_number,
    },
    model::{
        ArtifactRef, Capability, Change, DetectionResult, DetectionStatus, DoctorCheck,
        DoctorResult, DoctorStatus, EnvironmentRef, ExecutionPlan, Finding, IntegrationAction,
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

const INTEGRATION_ID: &str = "restic";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;

#[derive(Debug, Clone)]
pub struct ResticIntegration {
    descriptor: IntegrationDescriptor,
    executable: PathBuf,
    timeout_seconds: u64,
}

impl Default for ResticIntegration {
    fn default() -> Self {
        Self::new("restic", DEFAULT_TIMEOUT_SECONDS)
    }
}

impl ResticIntegration {
    pub fn new(executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(INTEGRATION_ID).expect("static restic integration id"),
                display_name: "restic".into(),
                level: IntegrationLevel::Semantic,
                capabilities: vec![
                    Capability::new("doctor", RiskClass::Read, false),
                    Capability::new("snapshots", RiskClass::Read, false),
                    Capability::new("backup", RiskClass::NetworkWrite, false),
                    Capability::new("check", RiskClass::Read, false),
                    Capability::new("forget", RiskClass::Destructive, false),
                    Capability::new("prune", RiskClass::Destructive, false),
                ],
            },
            executable: executable.into(),
            timeout_seconds,
        }
    }

    pub fn id(&self) -> &IntegrationId {
        &self.descriptor.id
    }

    fn environment_refs(&self, action: &IntegrationAction) -> Result<Vec<EnvironmentRef>> {
        if action.action == "doctor" {
            return Ok(Vec::new());
        }
        let mut refs = Vec::new();
        let repository = action
            .parameters
            .get("repository_env")
            .and_then(Value::as_str)
            .unwrap_or("RESTIC_REPOSITORY");
        refs.push(EnvironmentRef::new(repository, true)?);
        let password = action
            .parameters
            .get("password_env")
            .and_then(Value::as_str)
            .unwrap_or("RESTIC_PASSWORD_FILE");
        refs.push(EnvironmentRef::new(password, true)?);
        Ok(refs)
    }

    fn command(&self, action: &IntegrationAction) -> Result<CommandSpec> {
        let mut args = match action.action.as_str() {
            "doctor" => vec!["version".into()],
            "snapshots" => vec!["snapshots".into(), "--json".into()],
            "backup" => {
                let path = required_path(action, "path")?;
                vec!["backup".into(), "--json".into(), path]
            }
            "check" => vec!["check".into(), "--json".into()],
            "forget" => vec!["forget".into(), "--json".into()],
            "prune" => vec!["prune".into(), "--json".into()],
            unsupported => anyhow::bail!("unsupported restic action: {unsupported}"),
        };
        if matches!(action.action.as_str(), "forget" | "prune") {
            // Keep destructive operations deliberately narrow: selectors and
            // retention flags can be added only as typed parameters later.
            args.shrink_to_fit();
        }
        Ok(CommandSpec::argv(self.executable.clone(), args))
    }
}

impl Integration for ResticIntegration {
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
                detail: Some("restic executable was not found on PATH".into()),
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
                detail: Some(format!("restic version exited with {}", output.status)),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!("failed to execute restic: {error}")),
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
                name: "restic_cli".into(),
                ok: ready,
                detail: detection
                    .version
                    .unwrap_or_else(|| detection.detail.unwrap_or_default()),
            }],
        }
    }

    fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan> {
        let risk = match action.action.as_str() {
            "backup" => RiskClass::NetworkWrite,
            "forget" | "prune" => RiskClass::Destructive,
            "doctor" | "snapshots" | "check" => RiskClass::Read,
            unsupported => anyhow::bail!("unsupported restic action: {unsupported}"),
        };
        let plan = ExecutionPlan {
            integration: self.id().clone(),
            action: action.action.clone(),
            command: self.command(action)?,
            environment_refs: self.environment_refs(action)?,
            risk,
            requires_approval: risk.requires_approval(),
            supports_dry_run: false,
            dry_run: false,
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
            return Ok(failed_result(self.id(), &action.action, &output, "restic"));
        }
        if action.action == "doctor" {
            let version = first_line(output.stdout.as_bytes())
                .or_else(|| first_line(output.stderr.as_bytes()))
                .unwrap_or_else(|| "restic CLI responded successfully".into());
            return Ok(result(
                self.id(),
                &action.action,
                IntegrationStatus::Succeeded,
                safe_text(&version),
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ));
        }
        if action.action == "check" && output.stdout.trim().is_empty() {
            return Ok(result(
                self.id(),
                &action.action,
                IntegrationStatus::Succeeded,
                "restic repository check completed with no reported errors.",
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ));
        }
        let values = json_lines(&output.stdout)
            .with_context(|| format!("parse restic {} structured output", action.action))?;
        match action.action.as_str() {
            "snapshots" => parse_snapshots(self.id(), action, &values),
            "backup" => parse_operation(self.id(), action, &values, "backup"),
            "check" => parse_operation(self.id(), action, &values, "repository check"),
            "forget" => parse_operation(self.id(), action, &values, "forget"),
            "prune" => parse_operation(self.id(), action, &values, "prune"),
            unsupported => anyhow::bail!("unsupported restic action: {unsupported}"),
        }
    }

    fn verify(
        &self,
        action: &IntegrationAction,
        result: &IntegrationResult,
    ) -> Result<VerificationResult> {
        let failed = result.status == IntegrationStatus::Failed;
        let (status, detail) = match action.action.as_str() {
            "forget" | "prune" if !failed => (
                VerificationStatus::NotConfigured,
                "restic destructive state changes require a separate post-action verification contract.",
            ),
            "backup"
                if !failed
                    && result
                        .artifacts
                        .iter()
                        .any(|artifact| artifact.kind == "snapshot") =>
            {
                (
                    VerificationStatus::Passed,
                    "restic backup reported a snapshot identifier.",
                )
            }
            _ if !failed && result.findings.is_empty() => (
                VerificationStatus::Passed,
                "restic command completed without normalized errors.",
            ),
            _ if !failed => (
                VerificationStatus::Failed,
                "restic reported normalized errors.",
            ),
            _ => (
                VerificationStatus::Failed,
                "restic command did not complete successfully.",
            ),
        };
        Ok(VerificationResult {
            status,
            checks: vec![VerificationCheck {
                name: "restic_result".into(),
                status,
                detail: detail.into(),
            }],
        })
    }
}

fn required_path(action: &IntegrationAction, key: &str) -> Result<String> {
    let value = action
        .parameters
        .get(key)
        .and_then(Value::as_str)
        .context("restic backup requires a string parameter named path")?;
    if value.trim().is_empty() || value.starts_with('-') || value.contains('\0') {
        anyhow::bail!("restic backup path must be a non-empty non-option value");
    }
    Ok(value.into())
}

fn parse_snapshots(
    integration: &IntegrationId,
    action: &IntegrationAction,
    values: &[Value],
) -> Result<IntegrationResult> {
    let mut artifacts = Vec::new();
    let mut changes = Vec::new();
    for value in values.iter().take(200) {
        let Some(object) = value.as_object() else {
            continue;
        };
        let Some(id) = object
            .get("short_id")
            .or_else(|| object.get("id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let timestamp = object
            .get("time")
            .and_then(Value::as_str)
            .map(safe_text)
            .unwrap_or_else(|| "unknown-time".into());
        let reference = format!("{}@{}", safe_text(id), timestamp);
        artifacts.push(ArtifactRef {
            kind: "snapshot".into(),
            reference: reference.clone(),
        });
        changes.push(Change {
            kind: "snapshot".into(),
            description: format!("snapshot {reference}"),
            count: Some(1),
        });
    }
    let mut metrics = BTreeMap::new();
    metrics.insert(
        "snapshot_count".into(),
        metric(values.len() as f64, "count"),
    );
    Ok(result(
        integration,
        &action.action,
        IntegrationStatus::Succeeded,
        format!("restic returned {} snapshot(s).", values.len()),
        metrics,
        Vec::new(),
        changes,
        artifacts,
    ))
}

fn parse_operation(
    integration: &IntegrationId,
    action: &IntegrationAction,
    values: &[Value],
    label: &str,
) -> Result<IntegrationResult> {
    let mut metrics = BTreeMap::new();
    let mut changes = Vec::new();
    let mut artifacts = Vec::new();
    let mut findings = collect_error_findings(values, "restic");
    for value in values {
        let Some(object) = value.as_object() else {
            continue;
        };
        for key in [
            "files_new",
            "files_changed",
            "files_unmodified",
            "total_files_processed",
            "total_bytes_processed",
            "total_bytes_original",
            "data_added",
            "data_added_packed",
            "total_duration",
        ] {
            if let Some(raw) = object.get(key)
                && let Some(value) = string_number(raw)
            {
                let unit = if key.contains("bytes") || key.contains("data_") {
                    "bytes"
                } else if key == "total_duration" {
                    "seconds"
                } else {
                    "count"
                };
                metrics.insert(key.into(), metric(value, unit));
            }
        }
        if let Some(id) = object
            .get("snapshot_id")
            .or_else(|| object.get("snapshotID"))
            .and_then(Value::as_str)
        {
            let timestamp = object
                .get("time")
                .and_then(Value::as_str)
                .map(safe_text)
                .unwrap_or_else(|| "unknown-time".into());
            artifacts.push(ArtifactRef {
                kind: "snapshot".into(),
                reference: format!("{}@{}", safe_text(id), timestamp),
            });
        }
        for key in ["files_new", "files_changed", "files_unmodified"] {
            if let Some(count) = object.get(key).and_then(numeric) {
                changes.push(Change {
                    kind: key.into(),
                    description: format!("{key} during restic {label}"),
                    count: Some(count.max(0.0) as u64),
                });
            }
        }
        if object
            .get("message_type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.eq_ignore_ascii_case("error"))
            && object.get("error").is_none()
            && let Some(message) = object.get("message").and_then(Value::as_str)
        {
            findings.push(Finding {
                kind: "error".into(),
                title: format!("restic: {}", safe_text(message)),
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
    let summary = if action.action == "backup" {
        if let Some(snapshot) = artifacts.first() {
            format!("restic backup completed; snapshot {}.", snapshot.reference)
        } else {
            "restic backup completed without a reported snapshot identifier.".into()
        }
    } else {
        format!("restic {label} completed.")
    };
    Ok(result(
        integration,
        &action.action,
        status,
        summary,
        metrics,
        findings,
        changes,
        artifacts,
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
    fn descriptor_and_plans_cover_restic_risks_and_secret_refs() {
        let integration = ResticIntegration::new("restic", 60);
        assert_eq!(integration.descriptor().level, IntegrationLevel::Semantic);
        let backup = IntegrationAction::with_parameters(
            "backup",
            serde_json::json!({"path":"/tmp/source","password_env":"RESTIC_PASSWORD"}),
        )
        .unwrap();
        let plan = integration.plan(&backup).unwrap();
        assert_eq!(plan.command.args, ["backup", "--json", "/tmp/source"]);
        assert_eq!(plan.risk, RiskClass::NetworkWrite);
        assert_eq!(plan.environment_refs[1].name, "RESTIC_PASSWORD");
        let prune = integration
            .plan(&IntegrationAction::new("prune").unwrap())
            .unwrap();
        assert!(prune.requires_approval);
    }

    #[test]
    fn parses_snapshot_and_backup_semantics_without_raw_output() {
        let integration = ResticIntegration::default();
        let snapshots = IntegrationAction::new("snapshots").unwrap();
        let result = integration
            .parse(
                &snapshots,
                output(
                    ProcessStatus::Succeeded,
                    r#"[{"id":"abc123","time":"2026-08-12T00:00:00Z"}]"#,
                ),
            )
            .unwrap();
        assert_eq!(result.metrics["snapshot_count"].value, 1.0);
        assert!(result.artifacts[0].reference.contains("abc123"));

        let backup =
            IntegrationAction::with_parameters("backup", serde_json::json!({"path":"/tmp/source"}))
                .unwrap();
        let result = integration
            .parse(
                &backup,
                output(
                    ProcessStatus::Succeeded,
                    r#"{"message_type":"summary","snapshot_id":"abc123","files_new":2,"data_added":4096}"#,
                ),
            )
            .unwrap();
        assert_eq!(result.metrics["files_new"].value, 2.0);
        assert_eq!(result.metrics["data_added"].unit, "bytes");
        assert!(serde_json::to_string(&result).unwrap().contains("abc123"));
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("RESTIC_PASSWORD")
        );
    }

    #[test]
    fn handles_partial_json_errors_and_fail_closed_verification() {
        let integration = ResticIntegration::default();
        let check = IntegrationAction::new("check").unwrap();
        let result = integration
            .parse(
                &check,
                output(
                    ProcessStatus::Succeeded,
                    "{\"message_type\":\"status\"}\n{\"message_type\":\"error\",\"error\":\"repository mismatch\"}\nnot-json",
                ),
            )
            .unwrap();
        assert_eq!(result.status, IntegrationStatus::NeedsAttention);
        assert_eq!(
            integration.verify(&check, &result).unwrap().status,
            VerificationStatus::Failed
        );
        assert!(
            integration
                .parse(&check, output(ProcessStatus::Succeeded, "not-json"))
                .is_err()
        );
        let failed = integration
            .parse(&check, output(ProcessStatus::Failed, "token=do-not-echo"))
            .unwrap();
        assert!(
            !serde_json::to_string(&failed)
                .unwrap()
                .contains("do-not-echo")
        );
    }

    #[test]
    fn missing_executable_is_reported_without_spawning() {
        let detection = ResticIntegration::new("/definitely/missing/restic", 60).detect();
        assert_eq!(detection.status, DetectionStatus::Missing);
    }
}
