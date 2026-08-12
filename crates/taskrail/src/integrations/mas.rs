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
use anyhow::Result;
use std::{collections::BTreeMap, path::PathBuf, process::Command};

const INTEGRATION_ID: &str = "mas";
const DEFAULT_TIMEOUT_SECONDS: u64 = 10 * 60;

#[derive(Debug, Clone)]
pub struct MasIntegration {
    descriptor: IntegrationDescriptor,
    executable: PathBuf,
    timeout_seconds: u64,
}

impl Default for MasIntegration {
    fn default() -> Self {
        Self::new("mas", DEFAULT_TIMEOUT_SECONDS)
    }
}

impl MasIntegration {
    pub fn new(executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(INTEGRATION_ID).expect("static mas integration id"),
                display_name: "Mac App Store CLI".into(),
                level: IntegrationLevel::Semantic,
                capabilities: vec![
                    Capability::new("doctor", RiskClass::Read, false),
                    Capability::new("list", RiskClass::Read, false),
                    Capability::new("outdated", RiskClass::Read, false),
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
            "doctor" => vec!["version".into()],
            "list" => vec!["list".into()],
            "outdated" => vec!["outdated".into()],
            unsupported => anyhow::bail!("unsupported mas action: {unsupported}"),
        };
        Ok(CommandSpec::argv(self.executable.clone(), args))
    }
}

impl Integration for MasIntegration {
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
                detail: Some(
                    "mas executable was not found on PATH; this integration is macOS-only".into(),
                ),
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
                detail: Some(format!("mas version exited with {}", output.status)),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!("failed to execute mas: {error}")),
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
                name: "mas_cli".into(),
                ok: status == DoctorStatus::Ready,
                detail: detection
                    .version
                    .unwrap_or_else(|| detection.detail.unwrap_or_default()),
            }],
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
            return Ok(failed_result(self.id(), &action.action, &output, "mas"));
        }
        if action.action == "doctor" {
            let line = first_line(output.stdout.as_bytes())
                .unwrap_or_else(|| "mas CLI responded successfully".into());
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
        let mut changes = Vec::new();
        for line in output
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(200)
        {
            let mut parts = line.split_whitespace();
            let id = parts.next().unwrap_or("unknown");
            let description = safe_text(line);
            changes.push(Change {
                kind: action.action.clone(),
                description: format!("{id}: {description}"),
                count: Some(1),
            });
        }
        let mut metrics = BTreeMap::new();
        metrics.insert(
            "app_count".into(),
            super::helpers::metric(changes.len() as f64, "count"),
        );
        Ok(result(
            self.id(),
            &action.action,
            IntegrationStatus::Succeeded,
            format!("mas returned {} app record(s).", changes.len()),
            metrics,
            Vec::new(),
            changes,
            Vec::new(),
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
                name: "mas_read_model".into(),
                status,
                detail: "mas read-only output was bounded and normalized.".into(),
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
    fn plans_read_only_mas_actions_and_normalizes_list() {
        let integration = MasIntegration::new("mas", 60);
        let action = IntegrationAction::new("outdated").unwrap();
        assert_eq!(integration.plan(&action).unwrap().risk, RiskClass::Read);
        let result = integration
            .parse(
                &action,
                output(
                    ProcessStatus::Succeeded,
                    "123 App One 1.0 -> 2.0\n456 App Two 3.0 -> 4.0\n",
                ),
            )
            .unwrap();
        assert_eq!(result.metrics["app_count"].value, 2.0);
        assert!(result.changes[0].description.contains("123"));
    }

    #[test]
    fn failed_output_does_not_echo_secret_and_missing_mas_is_safe() {
        let integration = MasIntegration::default();
        let action = IntegrationAction::new("list").unwrap();
        let result = integration
            .parse(&action, output(ProcessStatus::Failed, "token=do-not-echo"))
            .unwrap();
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("do-not-echo")
        );
        assert_eq!(
            MasIntegration::new("/definitely/missing/mas", 60)
                .detect()
                .status,
            DetectionStatus::Missing
        );
    }
}
