use crate::{core::CommandSpec, executor};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct VerificationCommand {
    pub name: String,
    pub command: CommandSpec,
    pub expected_exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub name: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub status: String,
    pub cwd: PathBuf,
    pub checks: Vec<VerificationCheck>,
}

pub async fn run(
    cwd: impl AsRef<Path>,
    commands: Vec<VerificationCommand>,
    timeout_seconds: u64,
) -> Result<VerificationReport> {
    let cwd = cwd.as_ref().to_path_buf();
    if !cwd.is_dir() {
        anyhow::bail!("verification cwd is not a directory: {}", cwd.display());
    }
    let mut checks = Vec::with_capacity(commands.len());
    for item in commands {
        let mut command = item.command;
        if command.cwd.is_none() {
            command.cwd = Some(cwd.clone());
        }
        let result = executor::execute(&command, timeout_seconds).await?;
        let passed =
            result.exit_code == Some(item.expected_exit_code) && result.status == "succeeded";
        checks.push(VerificationCheck {
            name: item.name,
            status: if passed { "passed" } else { "failed" }.into(),
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
        });
    }
    Ok(VerificationReport {
        status: if checks.iter().all(|check| check.status == "passed") {
            "pass"
        } else {
            "fail"
        }
        .into(),
        cwd,
        checks,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[tokio::test]
    async fn passes_only_when_exit_code_matches() {
        let dir = tempdir().unwrap();
        let report = run(
            dir.path(),
            vec![VerificationCommand {
                name: "echo smoke".into(),
                command: CommandSpec::argv("/bin/echo", ["ok"]),
                expected_exit_code: 0,
            }],
            2,
        )
        .await
        .unwrap();
        assert_eq!(report.status, "pass");
        assert_eq!(report.checks[0].stdout.trim(), "ok");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reports_failed_verifier_without_claiming_success() {
        let dir = tempdir().unwrap();
        let report = run(
            dir.path(),
            vec![VerificationCommand {
                name: "false smoke".into(),
                command: CommandSpec::argv("/usr/bin/false", std::iter::empty::<String>()),
                expected_exit_code: 0,
            }],
            2,
        )
        .await
        .unwrap();
        assert_eq!(report.status, "fail");
        assert_eq!(report.checks[0].status, "failed");
    }
}
