use super::{
    helpers::{failed_result, first_line, metric, numeric, resolve_executable, result, safe_text},
    model::{
        Capability, Change, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult,
        DoctorStatus, ExecutionPlan, Finding, IntegrationAction, IntegrationDescriptor,
        IntegrationId, IntegrationLevel, IntegrationResult, IntegrationStatus, ProcessOutput,
        ProcessStatus, RiskClass, VerificationCheck, VerificationResult, VerificationStatus,
    },
    registry::Integration,
};
use crate::core::CommandSpec;
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, process::Command};

const INTEGRATION_ID: &str = "vibecleaner";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;
const SCRIPT_ENV: &str = "TASKRAIL_VIBECLEANER_SCRIPT";
const PYTHON_ENV: &str = "TASKRAIL_VIBECLEANER_PYTHON";
const MAX_DIRECTORIES: usize = 32;
const MAX_DIRECTORY_BYTES: usize = 4096;
const MAX_FINDINGS: usize = 128;
const MAX_MIN_SIZE_MB: u64 = 1_000_000;

/// How Taskrail launches VibeCleaner. The public VibeCleaner app is a GUI;
/// the typed adapter targets its documented headless CLI (or a compatible
/// wrapper) and never tries to drive the GUI.
#[derive(Debug, Clone)]
enum Launcher {
    Executable(PathBuf),
    PythonScript {
        interpreter: PathBuf,
        script: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub struct VibeCleanerIntegration {
    descriptor: IntegrationDescriptor,
    launcher: Launcher,
    timeout_seconds: u64,
}

impl Default for VibeCleanerIntegration {
    fn default() -> Self {
        let script = std::env::var_os(SCRIPT_ENV).filter(|value| !value.is_empty());
        let Some(script) = script else {
            return Self::new("vibecleaner", DEFAULT_TIMEOUT_SECONDS);
        };
        let interpreter = std::env::var_os(PYTHON_ENV)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "python3".into());
        Self::with_python(interpreter, script, DEFAULT_TIMEOUT_SECONDS)
    }
}

impl VibeCleanerIntegration {
    /// Construct an integration for an executable named `vibecleaner` or an
    /// equivalent wrapper that accepts `--cli ... --json`.
    pub fn new(executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: descriptor(),
            launcher: Launcher::Executable(executable.into()),
            timeout_seconds,
        }
    }

    /// Construct an integration for the upstream Python source CLI, whose
    /// documented invocation is `python source/vibecleaner.py --cli ... --json`.
    pub fn with_python(
        interpreter: impl Into<PathBuf>,
        script: impl Into<PathBuf>,
        timeout_seconds: u64,
    ) -> Self {
        Self {
            descriptor: descriptor(),
            launcher: Launcher::PythonScript {
                interpreter: interpreter.into(),
                script: script.into(),
            },
            timeout_seconds,
        }
    }

    pub fn id(&self) -> &IntegrationId {
        &self.descriptor.id
    }

    fn launcher_command(&self, args: impl IntoIterator<Item = String>) -> CommandSpec {
        match &self.launcher {
            Launcher::Executable(executable) => CommandSpec::argv(executable.clone(), args),
            Launcher::PythonScript {
                interpreter,
                script,
            } => {
                let args = std::iter::once(script.to_string_lossy().into_owned()).chain(args);
                CommandSpec::argv(interpreter.clone(), args)
            }
        }
    }

    fn resolved_command(&self, args: impl IntoIterator<Item = String>) -> Option<CommandSpec> {
        match &self.launcher {
            Launcher::Executable(executable) => {
                let executable = resolve_executable(executable)?;
                Some(CommandSpec::argv(executable, args))
            }
            Launcher::PythonScript {
                interpreter,
                script,
            } => {
                let interpreter = resolve_executable(interpreter)?;
                let script = script.is_file().then(|| script.to_path_buf())?;
                let args = std::iter::once(script.to_string_lossy().into_owned()).chain(args);
                Some(CommandSpec::argv(interpreter, args))
            }
        }
    }

    fn scan_command(&self, action: &IntegrationAction) -> Result<CommandSpec> {
        let directories = directories(action)?;
        let min_size_mb = min_size_mb(action)?;
        let mut args = vec!["--cli".into(), "--json".into()];
        if min_size_mb > 0 {
            args.extend(["--min-size".into(), min_size_mb.to_string()]);
        }
        // Keep directory paths positional even when a caller supplies a path
        // beginning with `-`; this remains direct argv, never a shell string.
        args.push("--".into());
        args.extend(directories);
        Ok(self.launcher_command(args))
    }
}

fn descriptor() -> IntegrationDescriptor {
    IntegrationDescriptor {
        id: IntegrationId::new(INTEGRATION_ID).expect("static VibeCleaner integration id"),
        display_name: "VibeCleaner".into(),
        level: IntegrationLevel::SafetyAware,
        capabilities: vec![Capability::new("scan", RiskClass::Read, false)],
    }
}

impl Integration for VibeCleanerIntegration {
    fn descriptor(&self) -> &IntegrationDescriptor {
        &self.descriptor
    }

    fn detect(&self) -> DetectionResult {
        let Some(command) = self.resolved_command(["--cli".into(), "--help".into()]) else {
            return DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Missing,
                executable: None,
                version: None,
                detail: Some(
                    "VibeCleaner headless CLI was not found; the GUI DMG does not provide a CLI path"
                        .into(),
                ),
            };
        };
        match Command::new(&command.executable)
            .args(&command.args)
            .output()
        {
            Ok(output) if output.status.success() => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Available,
                executable: Some(command.executable),
                version: None,
                detail: Some("VibeCleaner headless CLI responded to --help".into()),
            },
            Ok(output) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(command.executable),
                version: None,
                detail: Some(format!(
                    "VibeCleaner --cli --help exited with {}{}",
                    output.status,
                    first_line(&output.stderr)
                        .map(|line| format!(": {}", safe_text(&line)))
                        .unwrap_or_default()
                )),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(command.executable),
                version: None,
                detail: Some(format!("failed to execute VibeCleaner CLI: {error}")),
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
                name: "vibecleaner_cli".into(),
                ok: ready,
                detail: detection
                    .detail
                    .or(detection.version)
                    .unwrap_or_else(|| "VibeCleaner headless CLI is unavailable".into()),
            }],
        }
    }

    fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan> {
        if action.action != "scan" {
            anyhow::bail!("unsupported VibeCleaner action: {}", action.action);
        }
        let plan = ExecutionPlan {
            integration: self.id().clone(),
            action: action.action.clone(),
            command: self.scan_command(action)?,
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
                "VibeCleaner",
            ));
        }
        if action.action != "scan" {
            anyhow::bail!("unsupported VibeCleaner action: {}", action.action);
        }
        let value: Value = serde_json::from_str(&output.stdout)
            .context("VibeCleaner structured output is not valid JSON")?;
        parse_scan(self.id(), action, &value)
    }

    fn verify(
        &self,
        action: &IntegrationAction,
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
                name: "vibecleaner_scan_result".into(),
                status,
                detail: if status == VerificationStatus::Passed {
                    format!(
                        "VibeCleaner {} completed through the typed read-only path.",
                        action.action
                    )
                } else {
                    "VibeCleaner scan did not produce a successful result.".into()
                },
            }],
        })
    }
}

fn directories(action: &IntegrationAction) -> Result<Vec<String>> {
    let object = action
        .parameters
        .as_object()
        .context("VibeCleaner parameters must be a JSON object")?;
    reject_unknown_parameters(object, &["directories", "min_size_mb"])?;
    let values = object
        .get("directories")
        .and_then(Value::as_array)
        .context("VibeCleaner scan requires a non-empty directories array")?;
    if values.is_empty() || values.len() > MAX_DIRECTORIES {
        anyhow::bail!("VibeCleaner directories must contain between 1 and {MAX_DIRECTORIES} paths");
    }
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let path = value
                .as_str()
                .with_context(|| format!("VibeCleaner directories[{index}] must be a string"))?;
            if path.is_empty() || path.len() > MAX_DIRECTORY_BYTES || path.contains('\0') {
                anyhow::bail!(
                    "VibeCleaner directories[{index}] must be non-empty, NUL-free, and at most {MAX_DIRECTORY_BYTES} bytes"
                );
            }
            Ok(path.to_owned())
        })
        .collect()
}

fn min_size_mb(action: &IntegrationAction) -> Result<u64> {
    let Some(value) = action.parameters.get("min_size_mb") else {
        return Ok(0);
    };
    let value = value
        .as_u64()
        .context("VibeCleaner min_size_mb must be a non-negative integer")?;
    if value > MAX_MIN_SIZE_MB {
        anyhow::bail!("VibeCleaner min_size_mb must be at most {MAX_MIN_SIZE_MB}");
    }
    Ok(value)
}

fn reject_unknown_parameters(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<()> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        anyhow::bail!("unsupported VibeCleaner parameter: {key}");
    }
    Ok(())
}

fn parse_scan(
    integration: &IntegrationId,
    action: &IntegrationAction,
    value: &Value,
) -> Result<IntegrationResult> {
    let object = value
        .as_object()
        .context("VibeCleaner scan output must be a JSON object")?;
    let total_folders = object
        .get("total_folders")
        .and_then(Value::as_u64)
        .context("VibeCleaner scan output is missing total_folders")?;
    let total_bytes = object
        .get("total_bytes")
        .and_then(numeric)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .context("VibeCleaner scan output is missing total_bytes")?;
    let folders = object
        .get("folders")
        .and_then(Value::as_array)
        .context("VibeCleaner scan output is missing folders")?;
    if total_folders != folders.len() as u64 {
        anyhow::bail!("VibeCleaner scan output total_folders does not match folders length");
    }

    let mut safe_folders = 0u64;
    let mut verify_folders = 0u64;
    let mut unknown_folders = 0u64;
    let mut findings = Vec::new();
    for (index, folder) in folders.iter().enumerate() {
        let folder = folder
            .as_object()
            .with_context(|| format!("VibeCleaner folders[{index}] must be an object"))?;
        let name = folder
            .get("folder_name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .with_context(|| format!("VibeCleaner folders[{index}] is missing folder_name"))?;
        let risk = folder
            .get("risk")
            .and_then(Value::as_str)
            .filter(|risk| !risk.is_empty())
            .with_context(|| format!("VibeCleaner folders[{index}] is missing risk"))?;
        let path = folder
            .get("full_path")
            .and_then(Value::as_str)
            .map(safe_text);
        match risk.to_ascii_lowercase().as_str() {
            "safe" => safe_folders += 1,
            "verify" => {
                verify_folders += 1;
                if findings.len() < MAX_FINDINGS {
                    findings.push(Finding {
                        kind: "verify".into(),
                        title: format!("Verify before cleaning {}", safe_text(name)),
                        severity: Some("medium".into()),
                        location: path.clone(),
                        fingerprint: None,
                    });
                }
            }
            _ => {
                unknown_folders += 1;
                if findings.len() < MAX_FINDINGS {
                    findings.push(Finding {
                        kind: "unknown_risk".into(),
                        title: format!("Unknown VibeCleaner risk for {}", safe_text(name)),
                        severity: Some("high".into()),
                        location: path.clone(),
                        fingerprint: None,
                    });
                }
            }
        }
    }

    let mut metrics = BTreeMap::new();
    metrics.insert("total_bytes".into(), metric(total_bytes, "bytes"));
    metrics.insert(
        "total_folders".into(),
        metric(total_folders as f64, "count"),
    );
    metrics.insert("safe_folders".into(), metric(safe_folders as f64, "count"));
    metrics.insert(
        "verify_folders".into(),
        metric(verify_folders as f64, "count"),
    );
    metrics.insert(
        "unknown_folders".into(),
        metric(unknown_folders as f64, "count"),
    );
    let status = if unknown_folders > 0 {
        IntegrationStatus::NeedsAttention
    } else {
        IntegrationStatus::Succeeded
    };
    let summary = format!(
        "VibeCleaner found {total_folders} folder(s) reclaiming {} ({} safe, {} verify).",
        format_bytes(total_bytes),
        safe_folders,
        verify_folders
    );
    Ok(result(
        integration,
        &action.action,
        status,
        summary,
        metrics,
        findings,
        Vec::<Change>::new(),
        Vec::new(),
    ))
}

fn format_bytes(value: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = value;
    let mut index = 0;
    while value >= 1024.0 && index < UNITS.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{value:.0} {}", UNITS[index])
    } else {
        format!("{value:.2} {}", UNITS[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
    fn descriptor_exposes_only_read_only_scan() {
        let integration = VibeCleanerIntegration::default();
        assert_eq!(integration.descriptor().display_name, "VibeCleaner");
        assert_eq!(
            integration.descriptor().level,
            IntegrationLevel::SafetyAware
        );
        assert_eq!(integration.descriptor().capabilities.len(), 1);
        assert_eq!(integration.descriptor().capabilities[0].action, "scan");
        assert_eq!(
            integration.descriptor().capabilities[0].risk,
            RiskClass::Read
        );
    }

    #[test]
    fn plans_direct_argv_with_explicit_roots_and_size_filter() {
        let integration = VibeCleanerIntegration::new("vibecleaner", 60);
        let action = IntegrationAction::with_parameters(
            "scan",
            serde_json::json!({
                "directories": ["/Users/example/Projects", "--literal"],
                "min_size_mb": 500
            }),
        )
        .unwrap();
        let plan = integration.plan(&action).unwrap();
        assert_eq!(plan.risk, RiskClass::Read);
        assert!(!plan.requires_approval);
        assert_eq!(
            plan.command.args,
            [
                "--cli",
                "--json",
                "--min-size",
                "500",
                "--",
                "/Users/example/Projects",
                "--literal"
            ]
        );
        assert!(!plan.command.shell);
    }

    #[test]
    fn python_launcher_places_script_before_typed_arguments() {
        let integration = VibeCleanerIntegration::with_python("python3", "/tmp/vibecleaner.py", 60);
        let action = IntegrationAction::with_parameters(
            "scan",
            serde_json::json!({"directories":["/tmp/projects"]}),
        )
        .unwrap();
        let plan = integration.plan(&action).unwrap();
        assert_eq!(plan.command.executable, Path::new("python3"));
        assert_eq!(
            plan.command.args,
            [
                "/tmp/vibecleaner.py",
                "--cli",
                "--json",
                "--",
                "/tmp/projects"
            ]
        );
    }

    #[test]
    fn python_detection_accepts_non_executable_source_file() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("vibecleaner.py");
        std::fs::write(&script, "print('fixture')").unwrap();
        let interpreter = std::env::current_exe().unwrap();
        let integration = VibeCleanerIntegration::with_python(interpreter, &script, 60);
        assert!(
            integration
                .resolved_command(["--cli".into(), "--help".into()])
                .is_some()
        );
    }

    #[test]
    fn parses_metrics_and_verify_risk_without_fabricating_cleanup() {
        let integration = VibeCleanerIntegration::default();
        let action = IntegrationAction::with_parameters(
            "scan",
            serde_json::json!({"directories":["/tmp/projects"]}),
        )
        .unwrap();
        let result = integration
            .parse(
                &action,
                output(
                    ProcessStatus::Succeeded,
                    r#"{
                      "scan_root": ["/tmp/projects"],
                      "total_folders": 2,
                      "total_bytes": 3221225472,
                      "folders": [
                        {"folder_name":"node_modules","full_path":"/tmp/projects/a/node_modules","size_bytes":2147483648,"risk":"safe"},
                        {"folder_name":"dist","full_path":"/tmp/projects/b/dist","size_bytes":1073741824,"risk":"verify"}
                      ]
                    }"#,
                ),
            )
            .unwrap();
        assert_eq!(result.status, IntegrationStatus::Succeeded);
        assert_eq!(result.metrics["total_bytes"].unit, "bytes");
        assert_eq!(result.metrics["total_folders"].value, 2.0);
        assert_eq!(result.metrics["safe_folders"].value, 1.0);
        assert_eq!(result.metrics["verify_folders"].value, 1.0);
        assert_eq!(result.findings.len(), 1);
        assert!(result.findings[0].title.contains("dist"));
        assert!(result.summary.contains("3.00 GB"));
    }

    #[test]
    fn unknown_risk_is_attention_and_malformed_output_fails_closed() {
        let integration = VibeCleanerIntegration::default();
        let action = IntegrationAction::with_parameters(
            "scan",
            serde_json::json!({"directories":["/tmp/projects"]}),
        )
        .unwrap();
        let result = integration
            .parse(
                &action,
                output(
                    ProcessStatus::Succeeded,
                    r#"{"scan_root":["/tmp/projects"],"total_folders":1,"total_bytes":1,"folders":[{"folder_name":"cache","full_path":"/tmp/cache","risk":"new"}]}"#,
                ),
            )
            .unwrap();
        assert_eq!(result.status, IntegrationStatus::NeedsAttention);
        assert!(
            integration
                .parse(&action, output(ProcessStatus::Succeeded, "not-json"))
                .is_err()
        );
        assert!(
            integration
                .plan(
                    &IntegrationAction::with_parameters(
                        "scan",
                        serde_json::json!({"directories": []}),
                    )
                    .unwrap()
                )
                .is_err()
        );
    }

    #[test]
    fn failed_output_is_bounded_and_does_not_echo_secret() {
        let integration = VibeCleanerIntegration::default();
        let action = IntegrationAction::with_parameters(
            "scan",
            serde_json::json!({"directories":["/tmp/projects"]}),
        )
        .unwrap();
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
    fn missing_default_cli_is_reported_without_claiming_gui_support() {
        let integration =
            VibeCleanerIntegration::new("/definitely/missing/vibecleaner", DEFAULT_TIMEOUT_SECONDS);
        let detection = integration.detect();
        assert_eq!(detection.status, DetectionStatus::Missing);
        assert!(detection.detail.unwrap().contains("GUI DMG"));
        assert_eq!(integration.doctor().status, DoctorStatus::Unavailable);
    }
}
