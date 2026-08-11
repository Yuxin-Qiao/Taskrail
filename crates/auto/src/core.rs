use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    #[default]
    Observed,
    Adopted,
    Managed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    #[default]
    Enabled,
    Paused,
    Running,
    Degraded,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    #[default]
    R0Read,
    R1WorkspaceWrite,
    R2ExternalWrite,
    R3SystemWrite,
    R4Destructive,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    #[default]
    Pending,
    Approved,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub operation: String,
    pub risk: Risk,
    pub scope: serde_json::Value,
    pub state: ApprovalState,
    pub requested_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub actor: Option<String>,
}

impl ApprovalRequest {
    pub fn new(
        id: impl Into<String>,
        operation: impl Into<String>,
        risk: Risk,
        scope: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            operation: operation.into(),
            risk,
            scope,
            state: ApprovalState::Pending,
            requested_at: Utc::now(),
            resolved_at: None,
            actor: None,
        }
    }
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Self::R0Read => "R0_READ",
            Self::R1WorkspaceWrite => "R1_WORKSPACE_WRITE",
            Self::R2ExternalWrite => "R2_EXTERNAL_WRITE",
            Self::R3SystemWrite => "R3_SYSTEM_WRITE",
            Self::R4Destructive => "R4_DESTRUCTIVE",
        }
    }

    pub fn requires_approval(self) -> bool {
        self >= Self::R2ExternalWrite
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub shell: bool,
}

impl CommandSpec {
    pub fn argv(
        executable: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            executable: executable.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: None,
            env: BTreeMap::new(),
            shell: false,
        }
    }

    pub fn display(&self) -> String {
        std::iter::once(self.executable.to_string_lossy().into_owned())
            .chain(self.args.iter().cloned())
            .map(|part| shell_quote(&part))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn invokes_shell(&self) -> bool {
        let executable = self
            .executable
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        matches!(executable, "sh" | "bash" | "zsh" | "fish" | "dash" | "ksh")
            && self.args.iter().any(|arg| {
                arg == "-c"
                    || arg == "-ce"
                    || arg == "-ec"
                    || (arg.starts_with('-') && arg.contains('c'))
            })
    }
}

impl Default for CommandSpec {
    fn default() -> Self {
        Self {
            executable: PathBuf::new(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            shell: false,
        }
    }
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_./:@%+=,-".contains(c))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    #[default]
    Manual,
    Interval {
        seconds: u64,
    },
    Cron {
        expression: String,
        timezone: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MisfirePolicy {
    Skip,
    #[default]
    RunOnce,
    CatchUp {
        max_runs: u32,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcurrencyPolicy {
    Allow,
    #[default]
    ForbidOverlap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default)]
    pub initial_backoff_seconds: u64,
    #[serde(default = "default_max_backoff_seconds")]
    pub max_backoff_seconds: u64,
}

fn default_max_attempts() -> u32 {
    1
}

fn default_max_backoff_seconds() -> u64 {
    10 * 60
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            initial_backoff_seconds: 0,
            max_backoff_seconds: default_max_backoff_seconds(),
        }
    }
}

fn default_max_steps() -> u32 {
    100
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_steps: default_max_steps(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub max_risk: Risk,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default = "default_wall_time")]
    pub wall_time_seconds: u64,
    #[serde(default)]
    pub budget: Budget,
    #[serde(default)]
    pub retry: RetryPolicy,
}

fn default_wall_time() -> u64 {
    30 * 60
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_risk: Risk::R0Read,
            approval_required: false,
            wall_time_seconds: default_wall_time(),
            budget: Budget::default(),
            retry: RetryPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSpec {
    pub id: String,
    #[serde(default)]
    pub command: CommandSpec,
    #[serde(default)]
    pub responses: Option<ResponsesSpec>,
    #[serde(default)]
    pub risk: Risk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsesSpec {
    pub prompt: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    #[serde(default)]
    pub store: bool,
}

fn default_api_key_env() -> String {
    "OPENAI_API_KEY".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Automation {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub ownership: Ownership,
    #[serde(default)]
    pub runtime_state: RuntimeState,
    #[serde(default)]
    pub trigger: Trigger,
    #[serde(default)]
    pub misfire: MisfirePolicy,
    /// Optional upper bound for how old a due occurrence may be replayed.
    #[serde(default)]
    pub misfire_max_age_seconds: Option<u64>,
    #[serde(default)]
    pub concurrency: ConcurrencyPolicy,
    #[serde(default)]
    pub steps: Vec<StepSpec>,
    #[serde(default)]
    pub policy: Policy,
    #[serde(default = "default_revision")]
    pub revision: u64,
    pub source_id: Option<String>,
    pub fingerprint: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
}

fn default_revision() -> u64 {
    1
}

impl Default for Automation {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: String::new(),
            ownership: Ownership::Observed,
            runtime_state: RuntimeState::Enabled,
            trigger: Trigger::Manual,
            misfire: MisfirePolicy::default(),
            misfire_max_age_seconds: None,
            concurrency: ConcurrencyPolicy::default(),
            steps: Vec::new(),
            policy: Policy::default(),
            revision: 1,
            source_id: None,
            fingerprint: None,
            next_run_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredSource {
    pub source_id: String,
    pub provider: String,
    pub native_id: String,
    pub path: Option<PathBuf>,
    pub enabled: bool,
    pub kind: String,
    pub fingerprint: String,
    pub command: Option<CommandSpec>,
    pub trigger: Trigger,
    pub raw: String,
}

impl DiscoveredSource {
    pub fn as_observed_automation(&self) -> Option<Automation> {
        let command = self.command.clone()?;
        if command.shell || command.invokes_shell() {
            return None;
        }
        Some(Automation {
            id: self.source_id.clone(),
            name: self.native_id.clone(),
            ownership: Ownership::Observed,
            runtime_state: if self.enabled {
                RuntimeState::Enabled
            } else {
                RuntimeState::Paused
            },
            trigger: self.trigger.clone(),
            steps: vec![StepSpec {
                id: "main".to_owned(),
                risk: Risk::R0Read,
                command,
                responses: None,
            }],
            source_id: Some(self.source_id.clone()),
            fingerprint: Some(self.fingerprint.clone()),
            ..Automation::default()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub run_id: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub run_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub event_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub id: String,
    pub run_id: Option<String>,
    pub key: String,
    pub value: f64,
    pub unit: String,
    pub source: String,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptionState {
    Preparing,
    NativeDisabled,
    InternalEnabled,
    Committed,
    RollingBack,
    RolledBack,
    NeedsAttention,
}

impl AdoptionState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Committed | Self::RolledBack | Self::NeedsAttention
        )
    }
}

pub fn canonical_json<T: Serialize>(value: &T) -> anyhow::Result<String> {
    Ok(serde_json::to_string(value)?)
}

pub fn fingerprint_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_display_quotes_shell_metacharacters_without_enabling_shell() {
        let command = CommandSpec::argv("echo", ["hello world", "$(touch /tmp/pwned)"]);
        assert_eq!(
            command.display(),
            "echo 'hello world' '$(touch /tmp/pwned)'"
        );
        assert!(!command.shell);
    }

    #[test]
    fn discovered_source_becomes_observed_automation() {
        let source = DiscoveredSource {
            source_id: "cron:abc".into(),
            provider: "cron".into(),
            native_id: "daily".into(),
            path: None,
            enabled: true,
            kind: "task".into(),
            fingerprint: "sha256:test".into(),
            command: Some(CommandSpec::argv("echo", ["ok"])),
            trigger: Trigger::Interval { seconds: 60 },
            raw: "* * * * * echo ok".into(),
        };
        let automation = source.as_observed_automation().unwrap();
        assert_eq!(automation.ownership, Ownership::Observed);
        assert_eq!(automation.steps[0].command.args, ["ok"]);
    }

    #[test]
    fn shell_invoking_discovered_source_is_not_promoted_to_automation() {
        let source = DiscoveredSource {
            source_id: "launchd:shell".into(),
            provider: "launchd".into(),
            native_id: "shell".into(),
            path: None,
            enabled: true,
            kind: "task".into(),
            fingerprint: "sha256:shell".into(),
            command: Some(CommandSpec::argv("/bin/sh", ["-c", "echo unsafe"])),
            trigger: Trigger::Manual,
            raw: "shell".into(),
        };
        assert!(source.as_observed_automation().is_none());
    }

    #[test]
    fn policy_budget_defaults_for_older_definitions() {
        let policy: Policy = serde_yaml::from_str(
            "max_risk: r0_read\napproval_required: false\nwall_time_seconds: 5\n",
        )
        .unwrap();
        assert_eq!(policy.budget.max_steps, 100);
        assert_eq!(policy.retry.max_attempts, 1);
        assert_eq!(policy.retry.initial_backoff_seconds, 0);
        assert_eq!(policy.retry.max_backoff_seconds, 600);
    }

    #[test]
    fn deserializes_a_responses_api_step_without_a_command() {
        let automation: Automation = serde_yaml::from_str(
            r#"
id: api
name: api
ownership: managed
steps:
  - id: summarize
    responses:
      prompt: "Summarize the supplied state."
      base_url: "https://example.test/v1"
      model: "test-model"
      api_key_env: "TEST_API_KEY"
      store: false
"#,
        )
        .unwrap();
        assert!(
            automation.steps[0]
                .command
                .executable
                .as_os_str()
                .is_empty()
        );
        assert_eq!(
            automation.steps[0].responses.as_ref().unwrap().api_key_env,
            "TEST_API_KEY"
        );
    }
}
