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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSpec {
    pub id: String,
    #[serde(default)]
    pub command: CommandSpec,
    #[serde(default)]
    pub responses: Option<ResponsesSpec>,
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
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub retry: RetryPolicy,
    #[serde(default = "default_revision")]
    pub revision: u64,
    pub source_id: Option<String>,
    pub fingerprint: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
}

fn default_timeout_seconds() -> u64 {
    30 * 60
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
            timeout_seconds: default_timeout_seconds(),
            retry: RetryPolicy::default(),
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
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut fingerprint = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut fingerprint, "{byte:02x}").expect("writing a digest to String cannot fail");
    }
    format!("sha256:{fingerprint}")
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
    fn fingerprint_bytes_uses_stable_sha256_hex() {
        assert_eq!(
            fingerprint_bytes(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
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
