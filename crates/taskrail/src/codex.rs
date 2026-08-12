use crate::core::CommandSpec;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::{
    process::Command,
    time::{Duration, timeout},
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexSandbox {
    ReadOnly,
    WorkspaceWrite,
}

impl CodexSandbox {
    pub fn as_cli_arg(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodexRequest {
    pub cwd: PathBuf,
    pub prompt: String,
    pub sandbox: CodexSandbox,
    pub model: Option<String>,
    /// Optional Codex model catalog override for installations whose global
    /// catalog contains fields unsupported by the installed Codex CLI.
    pub model_catalog_json: Option<PathBuf>,
    pub output_schema: Option<PathBuf>,
    pub add_dirs: Vec<PathBuf>,
    pub timeout_seconds: u64,
}

impl CodexRequest {
    pub fn command_spec(&self) -> CommandSpec {
        self.command_spec_with_catalog(self.model_catalog_json.as_deref())
    }

    fn command_spec_with_catalog(&self, model_catalog_json: Option<&Path>) -> CommandSpec {
        let mut args = Vec::new();
        if let Some(catalog) = model_catalog_json {
            args.extend([
                "-c".to_owned(),
                format!("model_catalog_json={}", catalog.to_string_lossy()),
            ]);
        }
        args.extend([
            "exec".to_owned(),
            "--json".to_owned(),
            "--ephemeral".to_owned(),
            "--color".to_owned(),
            "never".to_owned(),
            "--sandbox".to_owned(),
            self.sandbox.as_cli_arg().to_owned(),
            "--cd".to_owned(),
            self.cwd.to_string_lossy().into_owned(),
        ]);
        if let Some(model) = &self.model {
            args.extend(["--model".to_owned(), model.clone()]);
        }
        if let Some(schema) = &self.output_schema {
            args.extend([
                "--output-schema".to_owned(),
                schema.to_string_lossy().into_owned(),
            ]);
        }
        for add_dir in &self.add_dirs {
            args.extend([
                "--add-dir".to_owned(),
                add_dir.to_string_lossy().into_owned(),
            ]);
        }
        args.push(self.prompt.clone());
        CommandSpec::argv("codex", args)
    }

    pub fn validate(&self) -> Result<()> {
        if !self.cwd.is_dir() {
            anyhow::bail!("Codex cwd is not a directory: {}", self.cwd.display());
        }
        if self.prompt.trim().is_empty() {
            anyhow::bail!("Codex prompt must not be empty");
        }
        if self.timeout_seconds == 0 {
            anyhow::bail!("Codex timeout must be greater than zero");
        }
        ensure_git_repository(&self.cwd)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexSummary {
    pub event_count: usize,
    pub thread_id: Option<String>,
    pub final_message: Option<String>,
    pub failure_types: Vec<String>,
    pub usage: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRunResult {
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout_jsonl: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub summary: CodexSummary,
}

pub async fn execute(request: &CodexRequest) -> Result<CodexRunResult> {
    request.validate()?;
    let temporary_catalog = temporary_compatible_catalog(request)?;
    let model_catalog_json = request.model_catalog_json.as_deref().or_else(|| {
        temporary_catalog
            .as_ref()
            .map(|catalog| catalog.path.as_path())
    });
    let command = request.command_spec_with_catalog(model_catalog_json);
    let start = Instant::now();
    let mut process = Command::new(&command.executable);
    process.kill_on_drop(true);
    process.args(&command.args).current_dir(&request.cwd);
    let output = match timeout(
        Duration::from_secs(request.timeout_seconds),
        process.output(),
    )
    .await
    {
        Ok(output) => output.with_context(|| format!("execute {}", command.display()))?,
        Err(_) => {
            return Ok(CodexRunResult {
                status: "timed_out".into(),
                exit_code: None,
                stdout_jsonl: String::new(),
                stderr: format!("codex exec exceeded {}s timeout", request.timeout_seconds),
                duration_ms: start.elapsed().as_millis(),
                summary: CodexSummary::default(),
            });
        }
    };
    let stdout_jsonl = String::from_utf8_lossy(&output.stdout).into_owned();
    let summary = parse_jsonl(&stdout_jsonl)?;
    let status = if output.status.success() && summary.failure_types.is_empty() {
        "succeeded"
    } else {
        "failed"
    };
    Ok(CodexRunResult {
        status: status.into(),
        exit_code: output.status.code(),
        stdout_jsonl,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        duration_ms: start.elapsed().as_millis(),
        summary,
    })
}

struct TemporaryCatalog {
    path: PathBuf,
}

impl Drop for TemporaryCatalog {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Keep a known cc-switch catalog incompatibility local to one Codex run.
/// The installed Codex CLI only accepts `text` and `image` input modalities,
/// while older generated catalogs may also advertise `audio`.
fn temporary_compatible_catalog(request: &CodexRequest) -> Result<Option<TemporaryCatalog>> {
    if request.model_catalog_json.is_some() {
        return Ok(None);
    }
    let Some(home) = std::env::var_os("HOME") else {
        return Ok(None);
    };
    let source = PathBuf::from(home).join(".codex/cc-switch-model-catalog.json");
    let raw = match fs::read_to_string(&source) {
        Ok(raw) => raw,
        Err(_) => return Ok(None),
    };
    let mut catalog: Value = match serde_json::from_str(&raw) {
        Ok(catalog) => catalog,
        Err(_) => return Ok(None),
    };
    if !strip_audio_modalities(&mut catalog) {
        return Ok(None);
    }

    let path = std::env::temp_dir().join(format!(
        "taskrail-codex-model-catalog-{}.json",
        Uuid::new_v4()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&path)
        .with_context(|| format!("create temporary Codex model catalog {}", path.display()))?;
    serde_json::to_writer_pretty(&mut file, &catalog)
        .context("write temporary Codex model catalog")?;
    file.write_all(b"\n")?;
    Ok(Some(TemporaryCatalog { path }))
}

fn strip_audio_modalities(value: &mut Value) -> bool {
    match value {
        Value::Object(object) => {
            let mut changed = false;
            if let Some(Value::Array(modalities)) = object.get_mut("input_modalities") {
                let before = modalities.len();
                modalities.retain(|modality| modality.as_str() != Some("audio"));
                changed |= before != modalities.len();
            }
            for child in object.values_mut() {
                changed |= strip_audio_modalities(child);
            }
            changed
        }
        Value::Array(values) => {
            let mut changed = false;
            for value in values {
                changed |= strip_audio_modalities(value);
            }
            changed
        }
        _ => false,
    }
}

pub fn parse_jsonl(stdout: &str) -> Result<CodexSummary> {
    let mut summary = CodexSummary::default();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line)
            .with_context(|| format!("parse Codex JSONL event: {line}"))?;
        summary.event_count += 1;
        match event.get("type").and_then(Value::as_str) {
            Some("thread.started") => {
                summary.thread_id = event
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }
            Some("turn.failed") | Some("error") => {
                if let Some(event_type) = event.get("type").and_then(Value::as_str) {
                    summary.failure_types.push(event_type.to_owned());
                }
            }
            Some("turn.completed") => summary.usage = event.get("usage").cloned(),
            Some("item.completed")
                if event.pointer("/item/type").and_then(Value::as_str) == Some("agent_message") =>
            {
                summary.final_message = event
                    .pointer("/item/text")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            _ => {}
        }
    }
    Ok(summary)
}

pub fn ensure_git_repository(path: &Path) -> Result<()> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &path.to_string_lossy(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .output()
        .context("check Git repository")?;
    if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "true" {
        anyhow::bail!(
            "Codex automation requires a Git repository: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn request() -> CodexRequest {
        CodexRequest {
            cwd: PathBuf::from("/tmp/repo"),
            prompt: "inspect the repository".into(),
            sandbox: CodexSandbox::ReadOnly,
            model: Some("gpt-5".into()),
            model_catalog_json: Some(PathBuf::from("/tmp/catalog.json")),
            output_schema: Some(PathBuf::from("schema.json")),
            add_dirs: vec![PathBuf::from("/tmp/shared")],
            timeout_seconds: 60,
        }
    }

    #[test]
    fn builds_documented_noninteractive_arguments_without_shell() {
        let command = request().command_spec();
        assert_eq!(command.executable, PathBuf::from("codex"));
        assert!(
            command
                .args
                .windows(2)
                .any(|pair| pair == ["--json", "--ephemeral"])
        );
        assert!(command.args.contains(&"--output-schema".into()));
        assert!(
            command
                .args
                .windows(2)
                .any(|pair| pair == ["-c", "model_catalog_json=/tmp/catalog.json"])
        );
        assert!(
            command.args.iter().position(|arg| arg == "-c").unwrap()
                < command.args.iter().position(|arg| arg == "exec").unwrap()
        );
        assert!(!command.shell);
    }

    #[test]
    fn parses_thread_message_and_failure_events() {
        let summary = parse_jsonl(
            r#"{"type":"thread.started","thread_id":"thread_1"}
{"type":"item.completed","item":{"type":"agent_message","text":"done"}}
{"type":"turn.completed","usage":{"input_tokens":10}}
"#,
        )
        .unwrap();
        assert_eq!(summary.event_count, 3);
        assert_eq!(summary.thread_id.as_deref(), Some("thread_1"));
        assert_eq!(summary.final_message.as_deref(), Some("done"));
        assert!(summary.failure_types.is_empty());
    }

    #[test]
    fn rejects_malformed_jsonl_instead_of_claiming_success() {
        assert!(parse_jsonl("not-json\n").is_err());
    }

    #[test]
    fn strips_only_unsupported_audio_modalities() {
        let mut value = serde_json::json!({
            "input_modalities": ["text", "image", "audio"],
            "nested": {"input_modalities": ["text", "audio"]},
            "other": ["audio"]
        });
        assert!(strip_audio_modalities(&mut value));
        assert_eq!(
            value["input_modalities"],
            serde_json::json!(["text", "image"])
        );
        assert_eq!(
            value["nested"]["input_modalities"],
            serde_json::json!(["text"])
        );
        assert_eq!(value["other"], serde_json::json!(["audio"]));
        assert!(!strip_audio_modalities(&mut value));
    }
}
