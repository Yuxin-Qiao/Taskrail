use super::model::{
    Capability, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult, DoctorStatus,
    ExecutionPlan, Finding, IntegrationAction, IntegrationDescriptor, IntegrationId,
    IntegrationLevel, IntegrationResult, IntegrationStatus, MetricValue, ProcessOutput,
    ProcessStatus, RiskClass, VerificationCheck, VerificationResult, VerificationStatus,
};
use super::registry::Integration;
use crate::core::CommandSpec;
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, env, path::PathBuf, process::Command};

const INTEGRATION_ID: &str = "mole";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;
const MAX_SAFE_TEXT: usize = 512;

#[derive(Debug, Clone)]
pub struct MoleIntegration {
    descriptor: IntegrationDescriptor,
    executable: PathBuf,
    timeout_seconds: u64,
}

impl Default for MoleIntegration {
    fn default() -> Self {
        Self::new("mo", DEFAULT_TIMEOUT_SECONDS)
    }
}

impl MoleIntegration {
    pub fn new(executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(INTEGRATION_ID).expect("static Mole integration id"),
                display_name: "Mole".into(),
                level: IntegrationLevel::Semantic,
                capabilities: vec![
                    Capability::new("detect", RiskClass::Read, false),
                    Capability::new("version", RiskClass::Read, false),
                    Capability::new("analyze", RiskClass::Read, false),
                    Capability::new("status", RiskClass::Read, false),
                    Capability::new("history", RiskClass::Read, false),
                    Capability::new("clean", RiskClass::Destructive, true),
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
        let mut args = match action.action.as_str() {
            "version" => vec!["--version".to_owned()],
            "analyze" => vec!["analyze".into(), "-json".into()],
            "status" => vec!["status".into(), "--json".into()],
            "history" => vec!["history".into(), "--json".into()],
            "clean" => vec!["clean".into()],
            action => anyhow::bail!("unsupported Mole action: {action}"),
        };
        if action.action == "history" {
            let limit = action
                .parameters
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(20)
                .clamp(1, 200);
            args.extend(["--limit".into(), limit.to_string()]);
        }
        if action.action == "clean"
            && action
                .parameters
                .get("dry_run")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            args.push("--dry-run".into());
        }
        Ok(CommandSpec::argv(self.executable.clone(), args))
    }

    fn action_is_dry_run(action: &IntegrationAction) -> bool {
        action.action == "clean"
            && action
                .parameters
                .get("dry_run")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    }
}

impl Integration for MoleIntegration {
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
                detail: Some("Mole executable was not found on PATH".into()),
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
                detail: Some(format!("Mole --version exited with {}", output.status)),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!("failed to execute Mole: {error}")),
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
                name: "mole_cli".into(),
                ok: ready,
                detail: detection
                    .version
                    .unwrap_or_else(|| detection.detail.unwrap_or_default()),
            }],
        }
    }

    fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan> {
        let dry_run = Self::action_is_dry_run(action);
        let risk = if action.action == "clean" && !dry_run {
            RiskClass::Destructive
        } else {
            RiskClass::Read
        };
        let plan = ExecutionPlan {
            integration: self.id().clone(),
            action: action.action.clone(),
            command: self.command(action)?,
            environment_refs: Vec::new(),
            risk,
            requires_approval: risk.requires_approval(),
            supports_dry_run: action.action == "clean",
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
            return Ok(IntegrationResult {
                integration: self.id().clone(),
                action: action.action.clone(),
                status: IntegrationStatus::Failed,
                summary: format!(
                    "Mole {} failed{}.",
                    action.action,
                    output
                        .exit_code
                        .map_or_else(String::new, |code| format!(" with exit code {code}"))
                ),
                metrics: BTreeMap::new(),
                findings: Vec::new(),
                changes: Vec::new(),
                artifacts: Vec::new(),
                raw_output_ref: None,
            });
        }
        match action.action.as_str() {
            "version" => parse_version(self.id(), action, &output.stdout),
            "analyze" | "status" | "history" => {
                let value: Value = serde_json::from_str(&output.stdout)
                    .context("Mole structured output is not valid JSON")?;
                parse_structured(self.id(), action, &value)
            }
            "clean" => Ok(IntegrationResult {
                integration: self.id().clone(),
                action: action.action.clone(),
                status: IntegrationStatus::Succeeded,
                summary: if Self::action_is_dry_run(action) {
                    "Mole cleanup dry-run completed.".into()
                } else {
                    "Mole cleanup completed.".into()
                },
                metrics: BTreeMap::new(),
                findings: Vec::new(),
                changes: Vec::new(),
                artifacts: Vec::new(),
                raw_output_ref: None,
            }),
            unsupported => anyhow::bail!("unsupported Mole action: {unsupported}"),
        }
    }

    fn verify(
        &self,
        action: &IntegrationAction,
        result: &IntegrationResult,
    ) -> Result<VerificationResult> {
        let dry_run = Self::action_is_dry_run(action);
        let deterministic = action.action != "clean" || dry_run;
        let command_succeeded = result.status != IntegrationStatus::Failed;
        Ok(VerificationResult {
            status: if command_succeeded && deterministic {
                VerificationStatus::Passed
            } else if command_succeeded {
                VerificationStatus::NotConfigured
            } else {
                VerificationStatus::Failed
            },
            checks: vec![VerificationCheck {
                name: if action.action == "clean" {
                    "mole_cleanup_state".into()
                } else {
                    "mole_command_status".into()
                },
                status: if command_succeeded && deterministic {
                    VerificationStatus::Passed
                } else if command_succeeded {
                    VerificationStatus::NotConfigured
                } else {
                    VerificationStatus::Failed
                },
                detail: if action.action == "clean" && !dry_run {
                    "Mole does not expose a deterministic post-clean state check in this adapter."
                        .into()
                } else {
                    "Mole command completed successfully.".into()
                },
            }],
        })
    }
}

fn parse_version(
    integration: &IntegrationId,
    action: &IntegrationAction,
    stdout: &str,
) -> Result<IntegrationResult> {
    let version =
        first_line(stdout.as_bytes()).context("Mole --version returned no version line")?;
    Ok(IntegrationResult {
        integration: integration.clone(),
        action: action.action.clone(),
        status: IntegrationStatus::Succeeded,
        summary: safe_text(&version),
        metrics: BTreeMap::new(),
        findings: Vec::new(),
        changes: Vec::new(),
        artifacts: Vec::new(),
        raw_output_ref: None,
    })
}

fn parse_structured(
    integration: &IntegrationId,
    action: &IntegrationAction,
    value: &Value,
) -> Result<IntegrationResult> {
    if !value.is_object() {
        anyhow::bail!("Mole {} output must be a JSON object", action.action);
    }
    let mut metrics = BTreeMap::new();
    collect_metrics(value, &mut metrics);
    let findings = collect_warnings(value);
    let summary = match action.action.as_str() {
        "history" => value.get("sessions").and_then(Value::as_array).map_or_else(
            || "Mole history completed.".into(),
            |sessions| format!("Mole history returned {} session(s).", sessions.len()),
        ),
        _ => known_summary(value).unwrap_or_else(|| format!("Mole {} completed.", action.action)),
    };
    let status = if findings.is_empty() {
        IntegrationStatus::Succeeded
    } else {
        IntegrationStatus::NeedsAttention
    };
    Ok(IntegrationResult {
        integration: integration.clone(),
        action: action.action.clone(),
        status,
        summary,
        metrics,
        findings,
        changes: Vec::new(),
        artifacts: Vec::new(),
        raw_output_ref: None,
    })
}

fn collect_metrics(value: &Value, metrics: &mut BTreeMap<String, MetricValue>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if let Some(number) = child.as_f64() {
                    if key.ends_with("_bytes") || key.contains("reclaim") {
                        metrics.insert(
                            key.clone(),
                            MetricValue {
                                value: number,
                                unit: "bytes".into(),
                            },
                        );
                    } else if key == "items" || key == "operation_count" {
                        metrics.insert(
                            key.clone(),
                            MetricValue {
                                value: number,
                                unit: "count".into(),
                            },
                        );
                    }
                } else if let Some(text) = child.as_str()
                    && (key.contains("reclaim") || key == "size")
                    && let Some(bytes) = parse_reported_bytes(text)
                {
                    metrics.insert(
                        format!("{key}_bytes"),
                        MetricValue {
                            value: bytes,
                            unit: "bytes".into(),
                        },
                    );
                }
                collect_metrics(child, metrics);
            }
        }
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_metrics(value, metrics)),
        _ => {}
    }
}

fn collect_warnings(value: &Value) -> Vec<Finding> {
    value
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|warning| Finding {
            kind: "warning".into(),
            title: safe_text(warning),
            severity: Some("medium".into()),
            location: None,
            fingerprint: None,
        })
        .collect()
}

fn known_summary(value: &Value) -> Option<String> {
    for key in ["summary", "status", "state", "message"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(safe_text(text));
        }
    }
    None
}

fn parse_reported_bytes(value: &str) -> Option<f64> {
    let value = value.trim();
    let split = value.find(|character: char| !(character.is_ascii_digit() || character == '.'))?;
    let number = value[..split].parse::<f64>().ok()?;
    let unit = value[split..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "b" => 1.0,
        "kb" | "kib" => 1024.0,
        "mb" | "mib" => 1024.0_f64.powi(2),
        "gb" | "gib" => 1024.0_f64.powi(3),
        "tb" | "tib" => 1024.0_f64.powi(4),
        _ => return None,
    };
    Some(number * multiplier)
}

fn resolve_executable(executable: &PathBuf) -> Option<PathBuf> {
    if executable.components().count() > 1 {
        return executable.is_file().then(|| executable.clone());
    }
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(executable))
        .find(|candidate| candidate.is_file())
}

fn first_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn safe_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if ["sk-", "token=", "password=", "secret=", "api_key="]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return "[REDACTED]".into();
    }
    let value = value.trim();
    if value.len() <= MAX_SAFE_TEXT {
        value.into()
    } else {
        let mut end = MAX_SAFE_TEXT;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &value[..end])
    }
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
            duration_ms: 12,
        }
    }

    #[test]
    fn descriptor_has_required_mole_capabilities_and_risks() {
        let integration = MoleIntegration::default();
        let capabilities = &integration.descriptor().capabilities;
        assert!(
            capabilities
                .iter()
                .any(|cap| cap.action == "analyze" && cap.risk == RiskClass::Read)
        );
        assert!(capabilities.iter().any(|cap| cap.action == "clean"
            && cap.risk == RiskClass::Destructive
            && cap.supports_dry_run));
    }

    #[test]
    fn plans_use_deterministic_argv_and_hold_real_clean() {
        let integration = MoleIntegration::new("mo", 60);
        let analyze = IntegrationAction::new("analyze").unwrap();
        let plan = integration.plan(&analyze).unwrap();
        assert_eq!(plan.command.args, ["analyze", "-json"]);
        assert_eq!(plan.risk, RiskClass::Read);
        let clean = IntegrationAction::new("clean").unwrap();
        assert_eq!(
            integration.plan(&clean).unwrap().risk,
            RiskClass::Destructive
        );
        let dry = IntegrationAction::with_parameters("clean", serde_json::json!({"dry_run":true}))
            .unwrap();
        let dry_plan = integration.plan(&dry).unwrap();
        assert_eq!(dry_plan.command.args, ["clean", "--dry-run"]);
        assert_eq!(dry_plan.risk, RiskClass::Read);
    }

    #[test]
    fn parses_reclaimable_bytes_without_fabricating_metrics() {
        let integration = MoleIntegration::default();
        let action = IntegrationAction::new("analyze").unwrap();
        let result = integration
            .parse(
                &action,
                output(
                    ProcessStatus::Succeeded,
                    r#"{"reclaimable":"1.5GB","warnings":[]}"#,
                ),
            )
            .unwrap();
        assert!((result.metrics["reclaimable_bytes"].value - 1.5 * 1024.0_f64.powi(3)).abs() < 1.0);
        assert!(
            integration
                .parse(&action, output(ProcessStatus::Succeeded, r#"{"other":42}"#))
                .unwrap()
                .metrics
                .is_empty()
        );
    }

    #[test]
    fn parses_history_and_warnings_as_bounded_semantics() {
        let integration = MoleIntegration::default();
        let action =
            IntegrationAction::with_parameters("history", serde_json::json!({"limit":2})).unwrap();
        let result = integration
            .parse(
                &action,
                output(
                    ProcessStatus::Succeeded,
                    r#"{"sessions":[{"size":"1.42GB"}],"warnings":["check disk"]}"#,
                ),
            )
            .unwrap();
        assert!(result.summary.contains("1 session"));
        assert_eq!(result.findings.len(), 1);
        assert!(
            integration
                .parse(&action, output(ProcessStatus::Succeeded, "not-json"))
                .is_err()
        );
    }

    #[test]
    fn handles_empty_and_nonzero_output_without_echoing_secrets() {
        let integration = MoleIntegration::default();
        let action = IntegrationAction::new("status").unwrap();
        assert!(
            integration
                .parse(&action, output(ProcessStatus::Succeeded, ""))
                .is_err()
        );
        let failed = integration
            .parse(
                &action,
                ProcessOutput {
                    status: ProcessStatus::Failed,
                    exit_code: Some(2),
                    stdout: String::new(),
                    stderr: "token=secret-value".into(),
                    duration_ms: 1,
                },
            )
            .unwrap();
        assert_eq!(failed.status, IntegrationStatus::Failed);
        assert!(
            !serde_json::to_string(&failed)
                .unwrap()
                .contains("secret-value")
        );
    }

    #[test]
    fn verification_is_fail_closed_for_real_clean() {
        let integration = MoleIntegration::default();
        let action = IntegrationAction::new("clean").unwrap();
        let result = integration
            .parse(&action, output(ProcessStatus::Succeeded, "completed"))
            .unwrap();
        assert_eq!(
            integration.verify(&action, &result).unwrap().status,
            VerificationStatus::NotConfigured
        );
    }
}
