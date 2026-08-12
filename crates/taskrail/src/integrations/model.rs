use crate::core::CommandSpec;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, path::PathBuf};

/// Stable identifier for a native integration, such as `mole` or `restic`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IntegrationId(String);

impl IntegrationId {
    pub fn new(value: impl Into<String>) -> anyhow::Result<Self> {
        let value = value.into();
        if value.trim().is_empty()
            || value.chars().any(|character| {
                !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
            })
        {
            anyhow::bail!("integration id must contain only letters, numbers, '-', '_' or '.'");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IntegrationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The maturity of an integration's semantic understanding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationLevel {
    Generic,
    Structured,
    SafetyAware,
    Semantic,
}

/// One supported action and its declared safety properties.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub action: String,
    pub risk: RiskClass,
    #[serde(default)]
    pub supports_dry_run: bool,
}

impl Capability {
    pub fn new(action: impl Into<String>, risk: RiskClass, supports_dry_run: bool) -> Self {
        Self {
            action: action.into(),
            risk,
            supports_dry_run,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationDescriptor {
    pub id: IntegrationId,
    pub display_name: String,
    pub level: IntegrationLevel,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationAction {
    pub action: String,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
}

impl IntegrationAction {
    pub fn new(action: impl Into<String>) -> anyhow::Result<Self> {
        let action = action.into();
        if action.trim().is_empty() {
            anyhow::bail!("integration action must not be empty");
        }
        Ok(Self {
            action,
            parameters: Value::Object(serde_json::Map::new()),
            approval_id: None,
        })
    }

    pub fn with_parameters(action: impl Into<String>, parameters: Value) -> anyhow::Result<Self> {
        let mut result = Self::new(action)?;
        result.parameters = parameters;
        Ok(result)
    }

    pub fn with_approval(mut self, approval_id: impl Into<String>) -> Self {
        self.approval_id = Some(approval_id.into());
        self
    }

    pub fn without_approval(mut self) -> Self {
        self.approval_id = None;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    Read,
    FilesystemWrite,
    NetworkWrite,
    SystemWrite,
    Destructive,
}

impl RiskClass {
    pub fn requires_approval(self) -> bool {
        !matches!(self, Self::Read)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionStatus {
    Available,
    Missing,
    Broken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub integration: IntegrationId,
    pub status: DetectionStatus,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    pub detail: Option<String>,
}

impl DetectionResult {
    pub fn available(
        integration: IntegrationId,
        executable: PathBuf,
        version: Option<String>,
    ) -> Self {
        Self {
            integration,
            status: DetectionStatus::Available,
            executable: Some(executable),
            version,
            detail: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Ready,
    NeedsConfiguration,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorResult {
    pub integration: IntegrationId,
    pub status: DoctorStatus,
    pub checks: Vec<DoctorCheck>,
}

/// A reference to an environment variable, never the variable's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentRef {
    pub name: String,
    #[serde(default)]
    pub required: bool,
}

impl EnvironmentRef {
    pub fn new(name: impl Into<String>, required: bool) -> anyhow::Result<Self> {
        let name = name.into();
        if name.trim().is_empty()
            || name
                .chars()
                .any(|character| !(character.is_ascii_alphanumeric() || character == '_'))
            || name
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
        {
            anyhow::bail!("environment reference must be a valid variable name");
        }
        Ok(Self { name, required })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationPlan {
    pub checks: Vec<VerificationCommandPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCommandPlan {
    pub name: String,
    pub command: CommandSpec,
    pub expected_exit_code: i32,
}

/// Deterministic argv plus safety metadata. The integration never executes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub integration: IntegrationId,
    pub action: String,
    pub command: CommandSpec,
    #[serde(default)]
    pub environment_refs: Vec<EnvironmentRef>,
    pub risk: RiskClass,
    pub requires_approval: bool,
    pub supports_dry_run: bool,
    #[serde(default)]
    pub dry_run: bool,
    pub timeout_seconds: u64,
    pub verification: Option<VerificationPlan>,
}

impl ExecutionPlan {
    /// Stable, secret-safe identity used to bind a persisted approval to the
    /// exact integration plan that may later be executed.
    pub fn fingerprint(&self) -> anyhow::Result<String> {
        Ok(crate::core::fingerprint_bytes(&serde_json::to_vec(
            &self.redacted(),
        )?))
    }

    pub fn redacted(&self) -> Self {
        let mut plan = self.clone();
        for value in plan.command.env.values_mut() {
            *value = "[REDACTED]".into();
        }
        plan
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.command.executable.as_os_str().is_empty() {
            anyhow::bail!("integration execution plan has no executable");
        }
        if self.command.shell || self.command.invokes_shell() {
            anyhow::bail!("integration plans must use direct argv commands");
        }
        if self.timeout_seconds == 0 {
            anyhow::bail!("integration execution plan timeout must be greater than zero");
        }
        if self.risk.requires_approval() && !self.requires_approval {
            anyhow::bail!("non-read integration actions must declare that approval is required");
        }
        if self.dry_run && !self.supports_dry_run {
            anyhow::bail!("integration action does not support dry-run");
        }
        for environment in &self.environment_refs {
            EnvironmentRef::new(environment.name.clone(), environment.required)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessStatus {
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessOutput {
    pub status: ProcessStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

impl ProcessOutput {
    pub fn bounded(mut self, max_output_bytes: usize) -> Self {
        self.stdout = bound_text(&self.stdout, max_output_bytes);
        self.stderr = bound_text(&self.stderr, max_output_bytes);
        self
    }
}

fn bound_text(value: &str, max_output_bytes: usize) -> String {
    if value.len() <= max_output_bytes {
        return value.to_owned();
    }
    let mut end = max_output_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[taskrail: integration output truncated]\n",
        &value[..end]
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationStatus {
    Succeeded,
    Failed,
    NeedsAttention,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue {
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub kind: String,
    pub title: String,
    pub severity: Option<String>,
    pub location: Option<String>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub kind: String,
    pub description: String,
    pub count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub kind: String,
    pub reference: String,
}

/// Bounded semantic output. Raw subprocess output is intentionally absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationResult {
    pub integration: IntegrationId,
    pub action: String,
    pub status: IntegrationStatus,
    pub summary: String,
    #[serde(default)]
    pub metrics: BTreeMap<String, MetricValue>,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub changes: Vec<Change>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactRef>,
    pub raw_output_ref: Option<String>,
}

impl IntegrationResult {
    pub fn ensure_matches(
        &self,
        integration: &IntegrationId,
        action: &IntegrationAction,
    ) -> anyhow::Result<()> {
        if &self.integration != integration || self.action != action.action {
            anyhow::bail!("integration result does not match the requested action");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
    NotConfigured,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub name: String,
    pub status: VerificationStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub checks: Vec<VerificationCheck>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_plan_rejects_shell_and_unapproved_write() {
        let integration = IntegrationId::new("fixture").unwrap();
        let mut plan = ExecutionPlan {
            integration,
            action: "clean".into(),
            command: CommandSpec::argv("/bin/echo", ["ok"]),
            environment_refs: Vec::new(),
            risk: RiskClass::Destructive,
            requires_approval: false,
            supports_dry_run: true,
            dry_run: false,
            timeout_seconds: 30,
            verification: None,
        };
        assert!(plan.validate().is_err());
        plan.requires_approval = true;
        plan.command = CommandSpec::argv("/bin/sh", ["-c", "echo unsafe"]);
        assert!(plan.validate().is_err());
    }

    #[test]
    fn process_output_is_bounded_on_utf8_boundaries() {
        let output = ProcessOutput {
            status: ProcessStatus::Succeeded,
            exit_code: Some(0),
            stdout: "🙂🙂🙂".into(),
            stderr: String::new(),
            duration_ms: 1,
        }
        .bounded(5);
        assert!(output.stdout.len() <= 64);
        assert!(output.stdout.contains("truncated"));
        assert!(std::str::from_utf8(output.stdout.as_bytes()).is_ok());
    }

    #[test]
    fn environment_reference_never_contains_a_secret_value() {
        let reference = EnvironmentRef::new("RESTIC_PASSWORD", true).unwrap();
        let serialized = serde_json::to_string(&reference).unwrap();
        assert_eq!(serialized, r#"{"name":"RESTIC_PASSWORD","required":true}"#);
        assert!(EnvironmentRef::new("bad=value", true).is_err());
    }

    #[test]
    fn normalized_result_contains_semantics_not_raw_output() {
        let result = IntegrationResult {
            integration: IntegrationId::new("fixture").unwrap(),
            action: "scan".into(),
            status: IntegrationStatus::Succeeded,
            summary: "2 findings".into(),
            metrics: BTreeMap::from([(
                "findings".into(),
                MetricValue {
                    value: 2.0,
                    unit: "count".into(),
                },
            )]),
            findings: vec![Finding {
                kind: "secret".into(),
                title: "credential-like match".into(),
                severity: Some("high".into()),
                location: Some("src/config.rs:10".into()),
                fingerprint: Some("sha256:fixture".into()),
            }],
            changes: Vec::new(),
            artifacts: Vec::new(),
            raw_output_ref: None,
        };
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(serialized.contains(r#""raw_output_ref":null"#));
        assert!(serialized.contains("findings"));
        assert!(!serialized.contains("credential-value"));
    }

    #[test]
    fn approval_id_is_optional_and_never_part_of_the_plan_identity() {
        let action = IntegrationAction::new("scan").unwrap();
        let approved = action.clone().with_approval("approval_test");
        assert_ne!(
            serde_json::to_string(&action).unwrap(),
            serde_json::to_string(&approved).unwrap()
        );
        assert_eq!(approved.without_approval().action, "scan");
    }
}
