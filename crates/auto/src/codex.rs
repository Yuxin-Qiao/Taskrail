use crate::core::{CommandSpec, fingerprint_bytes};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::{
    process::Command,
    time::{Duration, timeout},
};

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
    pub output_schema: Option<PathBuf>,
    pub add_dirs: Vec<PathBuf>,
    pub timeout_seconds: u64,
}

impl CodexRequest {
    pub fn command_spec(&self) -> CommandSpec {
        let mut args = vec![
            "exec".to_owned(),
            "--json".to_owned(),
            "--ephemeral".to_owned(),
            "--color".to_owned(),
            "never".to_owned(),
            "--sandbox".to_owned(),
            self.sandbox.as_cli_arg().to_owned(),
            "--cd".to_owned(),
            self.cwd.to_string_lossy().into_owned(),
        ];
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

    pub fn approval_scope(&self) -> Value {
        serde_json::json!({
            "cwd": self.cwd,
            "prompt_sha256": fingerprint_bytes(self.prompt.as_bytes()),
            "sandbox": self.sandbox,
            "model": self.model,
            "output_schema": self.output_schema,
            "add_dirs": self.add_dirs,
        })
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
    let command = request.command_spec();
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
}
