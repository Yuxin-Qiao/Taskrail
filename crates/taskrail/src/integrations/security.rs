use super::{
    helpers::{failed_result, first_line, json_lines, resolve_executable, result, safe_text},
    model::{
        Capability, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult, DoctorStatus,
        ExecutionPlan, Finding, IntegrationAction, IntegrationDescriptor, IntegrationId,
        IntegrationLevel, IntegrationResult, IntegrationStatus, ProcessOutput, ProcessStatus,
        RiskClass, VerificationCheck, VerificationResult, VerificationStatus,
    },
    registry::Integration,
};
use crate::{
    core::{CommandSpec, fingerprint_bytes},
    integrations::helpers::metric,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, process::Command};

const DEFAULT_TIMEOUT_SECONDS: u64 = 30 * 60;
const MAX_FINDINGS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityTool {
    Osv,
    Gitleaks,
    Trivy,
}

impl SecurityTool {
    fn id(self) -> &'static str {
        match self {
            Self::Osv => "osv-scanner",
            Self::Gitleaks => "gitleaks",
            Self::Trivy => "trivy",
        }
    }
    fn display_name(self) -> &'static str {
        match self {
            Self::Osv => "OSV-Scanner",
            Self::Gitleaks => "Gitleaks",
            Self::Trivy => "Trivy",
        }
    }
    fn executable(self) -> &'static str {
        match self {
            Self::Osv => "osv-scanner",
            Self::Gitleaks => "gitleaks",
            Self::Trivy => "trivy",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecurityIntegration {
    descriptor: IntegrationDescriptor,
    tool: SecurityTool,
    executable: PathBuf,
    timeout_seconds: u64,
}

impl SecurityIntegration {
    pub fn new(tool: SecurityTool, executable: impl Into<PathBuf>, timeout_seconds: u64) -> Self {
        Self {
            descriptor: IntegrationDescriptor {
                id: IntegrationId::new(tool.id()).expect("static scanner id"),
                display_name: tool.display_name().into(),
                level: IntegrationLevel::Semantic,
                capabilities: vec![
                    Capability::new("doctor", RiskClass::Read, false),
                    Capability::new("scan", RiskClass::Read, false),
                ],
            },
            tool,
            executable: executable.into(),
            timeout_seconds,
        }
    }

    pub fn for_tool(tool: SecurityTool) -> Self {
        Self::new(tool, tool.executable(), DEFAULT_TIMEOUT_SECONDS)
    }
    pub fn osv() -> Self {
        Self::for_tool(SecurityTool::Osv)
    }
    pub fn gitleaks() -> Self {
        Self::for_tool(SecurityTool::Gitleaks)
    }
    pub fn trivy() -> Self {
        Self::for_tool(SecurityTool::Trivy)
    }
    pub fn id(&self) -> &IntegrationId {
        &self.descriptor.id
    }

    fn command(&self, action: &IntegrationAction) -> Result<CommandSpec> {
        if action.action == "doctor" {
            return Ok(CommandSpec::argv(self.executable.clone(), ["--version"]));
        }
        if action.action != "scan" {
            anyhow::bail!("unsupported {} action: {}", self.tool.id(), action.action);
        }
        let path = required_path(action)?;
        let args = match self.tool {
            SecurityTool::Osv => vec![
                "scan".into(),
                "source".into(),
                "-r".into(),
                "--format".into(),
                "json".into(),
                path,
            ],
            SecurityTool::Gitleaks => {
                let mut args = vec![
                    "dir".into(),
                    "--report-format".into(),
                    "json".into(),
                    "--report-path".into(),
                    "/dev/stdout".into(),
                    "--no-banner".into(),
                ];
                if let Some(baseline) = action.parameters.get("baseline").and_then(Value::as_str) {
                    validate_path(baseline, "baseline")?;
                    args.extend(["--baseline-path".into(), baseline.into()]);
                }
                args.push(path);
                args
            }
            SecurityTool::Trivy => {
                let scan_type = action
                    .parameters
                    .get("scan_type")
                    .and_then(Value::as_str)
                    .unwrap_or("filesystem");
                let subcommand = match scan_type {
                    "filesystem" => "fs",
                    "repository" => "repo",
                    _ => anyhow::bail!("Trivy scan_type must be filesystem or repository"),
                };
                vec![subcommand.into(), "--format".into(), "json".into(), path]
            }
        };
        Ok(CommandSpec::argv(self.executable.clone(), args))
    }
}

impl Integration for SecurityIntegration {
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
                detail: Some(format!(
                    "{} executable was not found on PATH",
                    self.tool.display_name()
                )),
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
                detail: Some(format!(
                    "{} --version exited with {}",
                    self.tool.display_name(),
                    output.status
                )),
            },
            Err(error) => DetectionResult {
                integration: self.id().clone(),
                status: DetectionStatus::Broken,
                executable: Some(executable),
                version: None,
                detail: Some(format!(
                    "failed to execute {}: {error}",
                    self.tool.display_name()
                )),
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
                name: "scanner_cli".into(),
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
        if action.action == "doctor" {
            if !matches!(output.status, ProcessStatus::Succeeded) {
                return Ok(failed_result(
                    self.id(),
                    &action.action,
                    &output,
                    self.tool.display_name(),
                ));
            }
            let line = first_line(output.stdout.as_bytes()).unwrap_or_else(|| {
                format!("{} CLI responded successfully", self.tool.display_name())
            });
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
        if !matches!(output.status, ProcessStatus::Succeeded) && output.stdout.trim().is_empty() {
            return Ok(failed_result(
                self.id(),
                &action.action,
                &output,
                self.tool.display_name(),
            ));
        }
        let values = match json_lines(&output.stdout) {
            Ok(values) => values,
            Err(_error) if !matches!(output.status, ProcessStatus::Succeeded) => {
                return Ok(failed_result(
                    self.id(),
                    &action.action,
                    &output,
                    self.tool.display_name(),
                ));
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("parse {} scan JSON output", self.tool.display_name())
                });
            }
        };
        let findings = match self.tool {
            SecurityTool::Osv => parse_osv(&values),
            SecurityTool::Gitleaks => parse_gitleaks(&values),
            SecurityTool::Trivy => parse_trivy(&values),
        };
        let findings = findings.into_iter().take(MAX_FINDINGS).collect::<Vec<_>>();
        let mut metrics = BTreeMap::new();
        metrics.insert(
            "finding_count".into(),
            metric(findings.len() as f64, "count"),
        );
        let status = if findings.is_empty() && matches!(output.status, ProcessStatus::Succeeded) {
            IntegrationStatus::Succeeded
        } else if findings.is_empty() {
            IntegrationStatus::Failed
        } else {
            IntegrationStatus::NeedsAttention
        };
        Ok(result(
            self.id(),
            &action.action,
            status,
            format!(
                "{} reported {} normalized finding(s).",
                self.tool.display_name(),
                findings.len()
            ),
            metrics,
            findings,
            Vec::new(),
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
        Ok(VerificationResult { status, checks: vec![VerificationCheck { name: "scanner_result".into(), status, detail: "Scanner output was bounded and normalized; findings are surfaced as attention items.".into() }] })
    }
}

fn required_path(action: &IntegrationAction) -> Result<String> {
    let path = action
        .parameters
        .get("path")
        .and_then(Value::as_str)
        .context("security scan requires a path parameter")?;
    validate_path(path, "scan path")?;
    Ok(path.into())
}
fn validate_path(path: &str, label: &str) -> Result<()> {
    if path.trim().is_empty() || path.starts_with('-') || path.contains('\0') {
        anyhow::bail!("{label} must be a non-empty non-option value");
    }
    Ok(())
}
fn finding(
    kind: &str,
    title: String,
    severity: Option<String>,
    location: Option<String>,
    fingerprint: String,
) -> Finding {
    Finding {
        kind: kind.into(),
        title: safe_text(&title),
        severity: severity.map(|value| safe_text(&value)),
        location: location.map(|value| safe_text(&value)),
        fingerprint: Some(format!("sha256:{fingerprint}")),
    }
}
fn fingerprint(parts: &[String]) -> String {
    fingerprint_bytes(parts.join("\0").as_bytes())
}

fn parse_osv(values: &[Value]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for value in values {
        for result_value in value
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let source = result_value
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("dependency");
            for package in result_value
                .get("packages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let package_name = package
                    .get("package")
                    .and_then(|item| item.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("package");
                let version = package
                    .get("package")
                    .and_then(|item| item.get("version"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                for vulnerability in package
                    .get("vulnerabilities")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let id = vulnerability
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("OSV finding");
                    let summary = vulnerability
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or(id);
                    let severity = vulnerability
                        .get("severity")
                        .and_then(Value::as_array)
                        .and_then(|items| items.first())
                        .and_then(|item| item.get("score"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let parts = vec![
                        id.into(),
                        package_name.into(),
                        version.into(),
                        source.into(),
                    ];
                    findings.push(finding(
                        "vulnerability",
                        format!("{id}: {summary}"),
                        severity,
                        Some(format!("{package_name}@{version}")),
                        fingerprint(&parts),
                    ));
                }
            }
        }
    }
    findings
}

fn parse_gitleaks(values: &[Value]) -> Vec<Finding> {
    values
        .iter()
        .map(|value| {
            let rule = value
                .get("RuleID")
                .or_else(|| value.get("ruleID"))
                .and_then(Value::as_str)
                .unwrap_or("secret");
            let file = value
                .get("File")
                .or_else(|| value.get("file"))
                .and_then(Value::as_str)
                .unwrap_or("unknown file");
            let line = value
                .get("StartLine")
                .or_else(|| value.get("startLine"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let description = value
                .get("Description")
                .or_else(|| value.get("description"))
                .and_then(Value::as_str)
                .unwrap_or("secret-like match");
            let parts = vec![rule.into(), file.into(), line.to_string()];
            finding(
                "secret",
                format!("{rule}: {description}"),
                Some("high".into()),
                Some(format!("{file}:{line}")),
                fingerprint(&parts),
            )
        })
        .collect()
}

fn parse_trivy(values: &[Value]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for value in values {
        for item in value
            .get("Results")
            .or_else(|| value.get("results"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let target = item
                .get("Target")
                .or_else(|| item.get("target"))
                .and_then(Value::as_str)
                .unwrap_or("target");
            for vulnerability in item
                .get("Vulnerabilities")
                .or_else(|| item.get("vulnerabilities"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let id = vulnerability
                    .get("VulnerabilityID")
                    .or_else(|| vulnerability.get("vulnerabilityID"))
                    .and_then(Value::as_str)
                    .unwrap_or("vulnerability");
                let package = vulnerability
                    .get("PkgName")
                    .or_else(|| vulnerability.get("pkgName"))
                    .and_then(Value::as_str)
                    .unwrap_or("package");
                let severity = vulnerability
                    .get("Severity")
                    .or_else(|| vulnerability.get("severity"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                findings.push(finding(
                    "vulnerability",
                    format!("{id}: {package}"),
                    severity,
                    Some(target.into()),
                    fingerprint(&[id.into(), package.into(), target.into()]),
                ));
            }
            for misconfiguration in item
                .get("Misconfigurations")
                .or_else(|| item.get("misconfigurations"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let id = misconfiguration
                    .get("ID")
                    .or_else(|| misconfiguration.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("misconfiguration");
                let title = misconfiguration
                    .get("Title")
                    .or_else(|| misconfiguration.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or(id);
                findings.push(finding(
                    "misconfiguration",
                    format!("{id}: {title}"),
                    Some("medium".into()),
                    Some(target.into()),
                    fingerprint(&[id.into(), target.into()]),
                ));
            }
            for secret in item
                .get("Secrets")
                .or_else(|| item.get("secrets"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let rule = secret
                    .get("RuleID")
                    .or_else(|| secret.get("ruleID"))
                    .and_then(Value::as_str)
                    .unwrap_or("secret");
                let title = secret
                    .get("Title")
                    .or_else(|| secret.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or(rule);
                findings.push(finding(
                    "secret",
                    format!("{rule}: {title}"),
                    Some("high".into()),
                    Some(target.into()),
                    fingerprint(&[rule.into(), target.into()]),
                ));
            }
            for license in item
                .get("Licenses")
                .or_else(|| item.get("licenses"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = license
                    .get("Name")
                    .or_else(|| license.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("license");
                findings.push(finding(
                    "license",
                    safe_text(name),
                    None,
                    Some(target.into()),
                    fingerprint(&[name.into(), target.into()]),
                ));
            }
        }
    }
    findings
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
    fn scanner_plans_are_direct_argv_and_paths_are_validated() {
        let osv = SecurityIntegration::osv();
        let action =
            IntegrationAction::with_parameters("scan", serde_json::json!({"path":"."})).unwrap();
        let plan = osv.plan(&action).unwrap();
        assert_eq!(plan.risk, RiskClass::Read);
        assert!(!plan.command.shell);
        assert!(
            osv.plan(
                &IntegrationAction::with_parameters("scan", serde_json::json!({"path":"-bad"}))
                    .unwrap()
            )
            .is_err()
        );
        let gitleaks = SecurityIntegration::gitleaks();
        let plan = gitleaks
            .plan(
                &IntegrationAction::with_parameters(
                    "scan",
                    serde_json::json!({"path":".","baseline":".gitleaks-baseline.json"}),
                )
                .unwrap(),
            )
            .unwrap();
        assert!(plan.command.args.contains(&"--baseline-path".into()));
    }

    #[test]
    fn normalizes_osv_gitleaks_and_trivy_without_secret_values() {
        let osv = SecurityIntegration::osv();
        let action =
            IntegrationAction::with_parameters("scan", serde_json::json!({"path":"."})).unwrap();
        let result = osv.parse(&action, output(ProcessStatus::Succeeded, r#"{"results":[{"source":"Cargo.lock","packages":[{"package":{"name":"demo","version":"1.0"},"vulnerabilities":[{"id":"CVE-2026-0001","summary":"bad parser"}]}]}]}"#)).unwrap();
        assert_eq!(result.status, IntegrationStatus::NeedsAttention);
        assert_eq!(result.metrics["finding_count"].value, 1.0);
        let gitleaks = SecurityIntegration::gitleaks();
        let result = gitleaks.parse(&action, output(ProcessStatus::Failed, r#"[{"RuleID":"aws-key","Description":"cloud key","File":"src/main.rs","StartLine":4,"Secret":"DO_NOT_PERSIST"}]"#)).unwrap();
        assert_eq!(result.status, IntegrationStatus::NeedsAttention);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("DO_NOT_PERSIST")
        );
        let trivy = SecurityIntegration::trivy();
        let result = trivy.parse(&action, output(ProcessStatus::Succeeded, r#"{"Results":[{"Target":"Cargo.lock","Vulnerabilities":[{"VulnerabilityID":"CVE-1","PkgName":"demo","Severity":"HIGH"}],"Misconfigurations":[{"ID":"CFG-1","Title":"unsafe setting"}],"Secrets":[{"RuleID":"secret-rule","Title":"credential"}],"Licenses":[{"Name":"MIT"}]}]}"#)).unwrap();
        assert_eq!(result.metrics["finding_count"].value, 4.0);
        assert!(result.findings.iter().any(|item| item.kind == "license"));
    }

    #[test]
    fn malformed_and_missing_scanner_output_fail_closed() {
        let integration = SecurityIntegration::trivy();
        let action =
            IntegrationAction::with_parameters("scan", serde_json::json!({"path":"."})).unwrap();
        assert!(
            integration
                .parse(&action, output(ProcessStatus::Succeeded, "not-json"))
                .is_err()
        );
        let failed = integration
            .parse(&action, output(ProcessStatus::Failed, "token=do-not-echo"))
            .unwrap();
        assert_eq!(failed.status, IntegrationStatus::Failed);
        assert!(
            !serde_json::to_string(&failed)
                .unwrap()
                .contains("do-not-echo")
        );
        assert_eq!(
            SecurityIntegration::new(SecurityTool::Osv, "/definitely/missing/osv-scanner", 60)
                .detect()
                .status,
            DetectionStatus::Missing
        );
    }
}
