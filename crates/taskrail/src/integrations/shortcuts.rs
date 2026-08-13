use super::{
    helpers::{failed_result, first_line, resolve_executable, result, safe_text},
    model::{
        Capability, Change, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult,
        DoctorStatus, ExecutionPlan, IntegrationAction, IntegrationDescriptor, IntegrationId,
        IntegrationLevel, IntegrationResult, IntegrationStatus, ProcessOutput, ProcessStatus,
        RiskClass, VerificationCheck, VerificationResult, VerificationStatus,
    },
    registry::Integration,
};
use crate::core::CommandSpec;
use crate::discovery::{DiscoveryProvider, ShortcutsProvider};
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

const INTEGRATION_ID: &str = "shortcuts";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;

/// Typed access to Apple Shortcuts.
///
/// A Shortcut is an application-owned program and may perform arbitrary
/// side-effects. The adapter therefore exposes only `run`, requires the
/// normal durable approval path, accepts UUIDs rather than free-form command
/// text, and never returns the Shortcut's raw output as a semantic result.
#[derive(Debug, Clone)]
pub struct ShortcutsIntegration {
    descriptor: IntegrationDescriptor,
    executable: PathBuf,
    timeout_seconds: u64,
}

impl Default for ShortcutsIntegration {
    fn default() -> Self {
        Self::new("shortcuts", DEFAULT_TIMEOUT_SECONDS)
    }
}

impl ShortcutsIntegration {
    pub fn new(executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(INTEGRATION_ID).expect("static Shortcuts integration id"),
                display_name: "Apple Shortcuts".into(),
                level: IntegrationLevel::SafetyAware,
                capabilities: vec![
                    Capability::new("doctor", RiskClass::Read, false),
                    Capability::new("run", RiskClass::SystemWrite, false),
                ],
            },
            executable: executable.into(),
            timeout_seconds,
        }
    }

    pub fn id(&self) -> &IntegrationId {
        &self.descriptor.id
    }

    pub fn canonical_shortcut_id(action: &IntegrationAction) -> Result<String> {
        let parameters = action
            .parameters
            .as_object()
            .context("Shortcuts parameters must be a JSON object")?;
        for key in parameters.keys() {
            if key != "shortcut_id" {
                anyhow::bail!("unsupported parameter {key} for Shortcuts run");
            }
        }
        let value = action
            .parameters
            .get("shortcut_id")
            .and_then(Value::as_str)
            .context("Shortcuts run requires parameters.shortcut_id")?;
        let id = Uuid::parse_str(value)
            .with_context(|| "parameters.shortcut_id must be a Shortcut UUID")?;
        Ok(id.hyphenated().to_string())
    }

    fn command(&self, action: &IntegrationAction) -> Result<CommandSpec> {
        match action.action.as_str() {
            "run" => {
                let shortcut_id = Self::canonical_shortcut_id(action)?;
                Ok(CommandSpec::argv(
                    self.executable.clone(),
                    ["run", shortcut_id.as_str()],
                ))
            }
            "doctor" => {
                let parameters = action
                    .parameters
                    .as_object()
                    .context("Shortcuts parameters must be a JSON object")?;
                if !parameters.is_empty() {
                    anyhow::bail!("Shortcuts doctor does not accept parameters");
                }
                Ok(CommandSpec::argv(
                    self.executable.clone(),
                    ["list", "--show-identifiers"],
                ))
            }
            unsupported => anyhow::bail!("unsupported Shortcuts action: {unsupported}"),
        }
    }
}

impl Integration for ShortcutsIntegration {
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
                detail: Some("Apple Shortcuts CLI is available only on macOS".into()),
            };
        };
        DetectionResult::available(self.id().clone(), executable, None)
    }

    fn doctor(&self) -> DoctorResult {
        let detection = self.detect();
        let (status, check) = match detection.status {
            DetectionStatus::Available => (
                DoctorStatus::Ready,
                DoctorCheck {
                    name: "shortcuts_cli".into(),
                    ok: true,
                    detail: "Shortcuts CLI is available; use a fresh discovery scan before running a UUID.".into(),
                },
            ),
            DetectionStatus::Missing => (
                DoctorStatus::Unavailable,
                DoctorCheck {
                    name: "shortcuts_cli".into(),
                    ok: false,
                    detail: detection.detail.unwrap_or_default(),
                },
            ),
            DetectionStatus::Broken => (
                DoctorStatus::NeedsConfiguration,
                DoctorCheck {
                    name: "shortcuts_cli".into(),
                    ok: false,
                    detail: detection.detail.unwrap_or_default(),
                },
            ),
        };
        DoctorResult {
            integration: self.id().clone(),
            status,
            checks: vec![check],
        }
    }

    fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan> {
        let plan = ExecutionPlan {
            integration: self.id().clone(),
            action: action.action.clone(),
            command: self.command(action)?,
            environment_refs: Vec::new(),
            risk: if action.action == "run" {
                RiskClass::SystemWrite
            } else {
                RiskClass::Read
            },
            requires_approval: action.action == "run",
            supports_dry_run: false,
            dry_run: false,
            plan_only: false,
            timeout_seconds: self.timeout_seconds,
            verification: None,
        };
        plan.validate()?;
        Ok(plan)
    }

    fn preflight(&self, action: &IntegrationAction) -> Result<()> {
        if action.action != "run" {
            return Ok(());
        }
        let shortcut_id = Self::canonical_shortcut_id(action)?;
        let discovered = ShortcutsProvider {
            listing: None,
            executable: self.executable.clone(),
        }
        .scan()
        .map_err(|_| anyhow::anyhow!("fresh Apple Shortcuts discovery failed"))?;
        if discovered.iter().any(|source| {
            source
                .source_id
                .strip_prefix("shortcuts:")
                .and_then(|value| Uuid::parse_str(value).ok())
                .is_some_and(|value| value.hyphenated().to_string() == shortcut_id)
        }) {
            return Ok(());
        }
        anyhow::bail!(
            "Shortcut {shortcut_id} was not present in the fresh discovery result; run taskrail_scan_native with source=shortcuts and choose a current UUID"
        )
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
                "Shortcuts",
            ));
        }
        match action.action.as_str() {
            "doctor" => Ok(result(
                self.id(),
                &action.action,
                IntegrationStatus::Succeeded,
                safe_text(
                    &first_line(output.stdout.as_bytes())
                        .unwrap_or_else(|| "Shortcuts CLI responded successfully".into()),
                ),
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )),
            "run" => {
                let shortcut_id = Self::canonical_shortcut_id(action)?;
                Ok(result(
                    self.id(),
                    &action.action,
                    IntegrationStatus::Succeeded,
                    "Shortcut run completed.",
                    BTreeMap::new(),
                    Vec::new(),
                    vec![Change {
                        kind: "shortcut_run".into(),
                        description: format!("Shortcut {shortcut_id} completed."),
                        count: Some(1),
                    }],
                    Vec::new(),
                ))
            }
            unsupported => anyhow::bail!("unsupported Shortcuts action: {unsupported}"),
        }
    }

    fn verify(
        &self,
        _action: &IntegrationAction,
        result: &IntegrationResult,
    ) -> Result<VerificationResult> {
        let status = if result.status == IntegrationStatus::Succeeded {
            VerificationStatus::Passed
        } else {
            VerificationStatus::Failed
        };
        Ok(VerificationResult {
            status,
            checks: vec![VerificationCheck {
                name: "shortcuts_exit_status".into(),
                status,
                detail: "Shortcuts reported a successful process exit; no action-body claims are inferred.".into(),
            }],
        })
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
            duration_ms: 5,
        }
    }

    #[test]
    fn plans_only_uuid_runs_as_system_writes() {
        let integration = ShortcutsIntegration::new("shortcuts", 60);
        let action = IntegrationAction::with_parameters(
            "run",
            serde_json::json!({"shortcut_id":"11111111-1111-4111-8111-111111111111"}),
        )
        .unwrap();
        let plan = integration.plan(&action).unwrap();
        assert_eq!(plan.risk, RiskClass::SystemWrite);
        assert!(plan.requires_approval);
        assert_eq!(
            plan.command.args,
            ["run", "11111111-1111-4111-8111-111111111111"]
        );
        assert!(
            ShortcutsIntegration::canonical_shortcut_id(
                &IntegrationAction::with_parameters(
                    "run",
                    serde_json::json!({"shortcut_id":"name"})
                )
                .unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn parses_run_without_returning_raw_shortcut_output() {
        let integration = ShortcutsIntegration::default();
        let action = IntegrationAction::with_parameters(
            "run",
            serde_json::json!({"shortcut_id":"11111111-1111-4111-8111-111111111111"}),
        )
        .unwrap();
        let result = integration
            .parse(&action, output(ProcessStatus::Succeeded, "private output"))
            .unwrap();
        assert_eq!(result.summary, "Shortcut run completed.");
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("private output")
        );
        assert_eq!(result.changes[0].count, Some(1));
    }

    #[test]
    fn missing_shortcuts_executable_is_safe() {
        let integration = ShortcutsIntegration::new("/definitely/missing/shortcuts", 60);
        assert_eq!(integration.detect().status, DetectionStatus::Missing);
        let action = IntegrationAction::with_parameters(
            "run",
            serde_json::json!({"shortcut_id":"11111111-1111-4111-8111-111111111111"}),
        )
        .unwrap();
        assert!(integration.preflight(&action).is_err());
    }
}
