use crate::core::{CommandSpec, RunResult};
use anyhow::{Context, Result};
use std::time::Instant;
use tokio::{
    process::Command,
    sync::watch,
    time::{Duration, timeout},
};
use uuid::Uuid;

const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

pub async fn execute(command: &CommandSpec, timeout_seconds: u64) -> Result<RunResult> {
    let (_sender, cancellation) = watch::channel(false);
    execute_with_cancellation(command, timeout_seconds, cancellation).await
}

pub async fn execute_with_cancellation(
    command: &CommandSpec,
    timeout_seconds: u64,
    mut cancellation: watch::Receiver<bool>,
) -> Result<RunResult> {
    if command.shell || command.invokes_shell() {
        anyhow::bail!("shell execution is disabled; use a direct argv command instead");
    }
    let run_id = format!("run_{}", Uuid::new_v4());
    let start = Instant::now();
    let mut process = Command::new(&command.executable);
    process.kill_on_drop(true);
    process.args(&command.args);
    if let Some(cwd) = &command.cwd {
        process.current_dir(cwd);
    }
    process.envs(&command.env);
    if *cancellation.borrow() {
        return Ok(cancelled_result(run_id, start, "cancelled before spawn"));
    }
    let output = tokio::select! {
        result = timeout(Duration::from_secs(timeout_seconds), process.output()) => {
            match result {
                Ok(result) => result.with_context(|| format!("execute {}", command.display()))?,
                Err(_) => {
                    return Ok(RunResult {
                        run_id,
                        status: "timed_out".into(),
                        exit_code: None,
                        stdout: String::new(),
                        stderr: format!("command exceeded {timeout_seconds}s timeout"),
                        duration_ms: start.elapsed().as_millis(),
                    });
                }
            }
        }
        changed = cancellation.changed() => {
            if changed.is_ok() && *cancellation.borrow() {
                return Ok(cancelled_result(run_id, start, "cancelled by supervisor"));
            }
            return Ok(cancelled_result(run_id, start, "cancellation channel closed"));
        }
    };
    if *cancellation.borrow() {
        return Ok(cancelled_result(run_id, start, "cancelled by supervisor"));
    }
    Ok(RunResult {
        run_id,
        status: if output.status.success() {
            "succeeded"
        } else {
            "failed"
        }
        .into(),
        exit_code: output.status.code(),
        stdout: redacted_output(&output.stdout, command.env.values()),
        stderr: redacted_output(&output.stderr, command.env.values()),
        duration_ms: start.elapsed().as_millis(),
    })
}

fn cancelled_result(run_id: String, start: Instant, reason: &str) -> RunResult {
    RunResult {
        run_id,
        status: "cancelled".into(),
        exit_code: None,
        stdout: String::new(),
        stderr: reason.into(),
        duration_ms: start.elapsed().as_millis(),
    }
}

fn bounded_output(bytes: &[u8]) -> String {
    if bytes.len() <= MAX_OUTPUT_BYTES {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut output = String::from_utf8_lossy(&bytes[..MAX_OUTPUT_BYTES]).into_owned();
    output.push_str("\n[taskrail: output truncated at 1 MiB]\n");
    output
}

fn redacted_output<'a>(bytes: &[u8], secrets: impl IntoIterator<Item = &'a String>) -> String {
    let mut output = bounded_output(bytes);
    for secret in secrets {
        if !secret.is_empty() {
            output = output.replace(secret, "[REDACTED]");
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CommandSpec;

    #[cfg(unix)]
    #[tokio::test]
    async fn executes_argv_without_shell_expansion() {
        let command = CommandSpec::argv("/bin/echo", ["$(printf injected)"]);
        let result = execute(&command, 2).await.unwrap();
        assert_eq!(result.status, "succeeded");
        assert_eq!(result.stdout.trim(), "$(printf injected)");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_explicit_shell_mode() {
        let mut command = CommandSpec::argv("echo", ["x"]);
        command.shell = true;
        let error = execute(&command, 1).await.unwrap_err().to_string();
        assert!(error.contains("shell execution is disabled"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_shell_invocation_hidden_in_argv() {
        let command = CommandSpec::argv("/bin/sh", ["-c", "echo unsafe"]);
        let error = execute(&command, 1).await.unwrap_err().to_string();
        assert!(error.contains("shell execution is disabled"));
    }

    #[test]
    fn bounds_captured_output() {
        let output = bounded_output(&vec![b'x'; MAX_OUTPUT_BYTES + 1]);
        assert!(output.len() < MAX_OUTPUT_BYTES + 128);
        assert!(output.contains("output truncated"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn redacts_environment_values_from_captured_output() {
        let mut command = CommandSpec::argv("/bin/echo", ["secret-value"]);
        command
            .env
            .insert("AUTO_TEST_SECRET".into(), "secret-value".into());
        let result = execute(&command, 2).await.unwrap();
        assert_eq!(result.status, "succeeded");
        assert!(!result.stdout.contains("secret-value"));
        assert!(result.stdout.contains("[REDACTED]"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_kills_a_running_process_and_reports_cancelled() {
        let (sender, receiver) = watch::channel(false);
        let task = tokio::spawn(async move {
            execute_with_cancellation(&CommandSpec::argv("/bin/sleep", ["10"]), 30, receiver)
                .await
                .unwrap()
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        sender.send(true).unwrap();
        let result = task.await.unwrap();
        assert_eq!(result.status, "cancelled");
    }
}
