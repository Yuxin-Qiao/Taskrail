use super::{
    helpers::{failed_result, first_line, resolve_executable, result, safe_text},
    model::{
        Capability, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult, DoctorStatus,
        ExecutionPlan, IntegrationAction, IntegrationDescriptor, IntegrationId, IntegrationLevel,
        IntegrationResult, IntegrationStatus, ProcessOutput, ProcessStatus, RiskClass,
        VerificationCheck, VerificationResult, VerificationStatus,
    },
    registry::Integration,
};
use crate::core::CommandSpec;
use anyhow::Result;
use std::{collections::BTreeMap, path::PathBuf, process::Command};

const INTEGRATION_ID: &str = "topgrade";
const DEFAULT_TIMEOUT_SECONDS: u64 = 60 * 60;

#[derive(Debug, Clone)]
pub struct TopgradeIntegration {
    descriptor: IntegrationDescriptor,
    executable: PathBuf,
    timeout_seconds: u64,
}

impl Default for TopgradeIntegration {
    fn default() -> Self {
        Self::new("topgrade", DEFAULT_TIMEOUT_SECONDS)
    }
}

impl TopgradeIntegration {
    pub fn new(executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(INTEGRATION_ID).expect("static topgrade integration id"),
                display_name: "Topgrade".into(),
                level: IntegrationLevel::SafetyAware,
                capabilities: vec![
                    Capability::new("doctor", RiskClass::Read, false),
                    Capability::new("inspect", RiskClass::Read, false),
                    Capability::new("plan", RiskClass::Read, false),
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

    fn command(&self, action: &IntegrationAction) -> Result<CommandSpec> {
        let args: Vec<String> = match action.action.as_str() {
            "doctor" => vec!["--version".into()],
            // Topgrade does not promise a stable machine-readable dry-run
            // contract across releases. The service marks these actions as
            // plan-only and never starts the executable.
            "inspect" | "plan" => Vec::new(),
            "run" => Vec::new(),
            unsupported => anyhow::bail!("unsupported Topgrade action: {unsupported}"),
        };
        Ok(CommandSpec::argv(self.executable.clone(), args))
    }
}

impl Integration for TopgradeIntegration {
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
                detail: Some("Topgrade executable was not found on PATH".into()),
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
                detail: Some(format!("topgrade --version exited with {}", output.status)),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!("failed to execute topgrade: {error}")),
            },
        }
    }

    fn doctor(&self) -> DoctorResult {
        let detection = self.detect();
        let status = match detection.status {
            DetectionStatus::Available => DoctorStatus::Ready,
            DetectionStatus::Missing => DoctorStatus::Unavailable,
            DetectionStatus::Broken => DoctorStatus::NeedsConfiguration,
        };
        DoctorResult {
            integration: self.id().clone(),
            status,
            checks: vec![DoctorCheck {
                name: "topgrade_cli".into(),
                ok: status == DoctorStatus::Ready,
                detail: detection
                    .version
                    .unwrap_or_else(|| detection.detail.unwrap_or_default()),
            }],
        }
    }

    fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan> {
        let risk = match action.action.as_str() {
            "doctor" | "inspect" | "plan" => RiskClass::Read,
            "run" => RiskClass::SystemWrite,
            unsupported => anyhow::bail!("unsupported Topgrade action: {unsupported}"),
        };
        let plan = ExecutionPlan {
            integration: self.id().clone(),
            action: action.action.clone(),
            command: self.command(action)?,
            environment_refs: Vec::new(),
            risk,
            requires_approval: risk.requires_approval(),
            supports_dry_run: false,
            dry_run: false,
            plan_only: matches!(action.action.as_str(), "inspect" | "plan"),
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
        if matches!(action.action.as_str(), "inspect" | "plan") {
            return Ok(result(
                self.id(),
                &action.action,
                IntegrationStatus::Succeeded,
                "Topgrade plan recorded; no Topgrade process was started because upstream has no stable plan-only CLI contract.",
                BTreeMap::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ));
        }
        if !matches!(output.status, ProcessStatus::Succeeded) {
            return Ok(failed_result(
                self.id(),
                &action.action,
                &output,
                "Topgrade",
            ));
        }
        if action.action == "doctor" {
            let line = first_line(output.stdout.as_bytes())
                .unwrap_or_else(|| "Topgrade CLI responded successfully".into());
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
        Ok(result(
            self.id(),
            &action.action,
            IntegrationStatus::Succeeded,
            "Topgrade run completed; inspect the bounded run log for tool details.",
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
        } else if action.action == "run" {
            VerificationStatus::NotConfigured
        } else {
            VerificationStatus::Passed
        };
        Ok(VerificationResult {
            status,
            checks: vec![VerificationCheck {
                name: "topgrade_result".into(),
                status,
                detail: if action.action == "run" {
                    "Topgrade has no stable deterministic post-update state contract in this adapter.".into()
                } else {
                    "Topgrade read-only plan/doctor path completed.".into()
                },
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
    fn read_only_plan_views_do_not_spawn_and_run_is_system_write() {
        let integration = TopgradeIntegration::new("topgrade", 60);
        for name in ["inspect", "plan"] {
            let plan = integration
                .plan(&IntegrationAction::new(name).unwrap())
                .unwrap();
            assert_eq!(plan.risk, RiskClass::Read);
            assert!(plan.command.args.is_empty());
            assert_eq!(plan.command.executable, PathBuf::from("topgrade"));
            assert!(plan.plan_only);
        }
        let run = integration
            .plan(&IntegrationAction::new("run").unwrap())
            .unwrap();
        assert_eq!(run.risk, RiskClass::SystemWrite);
        assert!(run.requires_approval);
    }

    #[test]
    fn normalizes_plan_and_fails_closed_for_run_verification() {
        let integration = TopgradeIntegration::default();
        let plan = IntegrationAction::new("plan").unwrap();
        let result = integration
            .parse(&plan, output(ProcessStatus::Succeeded, "ignored"))
            .unwrap();
        assert_eq!(
            integration.verify(&plan, &result).unwrap().status,
            VerificationStatus::Passed
        );
        let run = IntegrationAction::new("run").unwrap();
        let result = integration
            .parse(&run, output(ProcessStatus::Succeeded, "updated"))
            .unwrap();
        assert_eq!(
            integration.verify(&run, &result).unwrap().status,
            VerificationStatus::NotConfigured
        );
        let failed = integration
            .parse(&run, output(ProcessStatus::Failed, "token=do-not-echo"))
            .unwrap();
        assert!(
            !serde_json::to_string(&failed)
                .unwrap()
                .contains("do-not-echo")
        );
    }

    #[test]
    fn missing_topgrade_is_detected() {
        assert_eq!(
            TopgradeIntegration::new("/definitely/missing/topgrade", 60)
                .detect()
                .status,
            DetectionStatus::Missing
        );
    }
}
