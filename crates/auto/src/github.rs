use crate::{
    core::{CommandSpec, fingerprint_bytes},
    executor,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind {
    Issues,
    Pulls,
    FailedRuns,
    Checks,
}

#[derive(Debug, Clone)]
pub struct GhQuery {
    pub repo: String,
    pub kind: QueryKind,
    pub pull_number: Option<u64>,
}

impl GhQuery {
    pub fn watch_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.repo,
            self.kind.as_str(),
            self.pull_number
                .map_or_else(|| "-".into(), |value| value.to_string())
        )
    }

    pub fn command_spec(&self) -> Result<CommandSpec> {
        validate_repo(&self.repo)?;
        let command = match self.kind {
            QueryKind::Issues => CommandSpec::argv(
                "gh",
                [
                    "issue",
                    "list",
                    "-R",
                    &self.repo,
                    "--state",
                    "open",
                    "--json",
                    "number,title,labels,createdAt,updatedAt,url",
                ],
            ),
            QueryKind::Pulls => CommandSpec::argv(
                "gh",
                [
                    "pr",
                    "list",
                    "-R",
                    &self.repo,
                    "--state",
                    "open",
                    "--json",
                    "number,title,isDraft,reviewDecision,statusCheckRollup,url",
                ],
            ),
            QueryKind::FailedRuns => CommandSpec::argv(
                "gh",
                [
                    "run",
                    "list",
                    "-R",
                    &self.repo,
                    "--status",
                    "failure",
                    "--json",
                    "databaseId,name,workflowName,headBranch,status,conclusion,createdAt,url",
                ],
            ),
            QueryKind::Checks => {
                let number = self
                    .pull_number
                    .context("checks query requires --pull-number")?;
                CommandSpec::argv(
                    "gh",
                    [
                        "pr",
                        "checks",
                        &number.to_string(),
                        "-R",
                        &self.repo,
                        "--json",
                        "name,state,bucket,link",
                    ],
                )
            }
        };
        Ok(command)
    }

    pub fn command_spec_with_executable(
        &self,
        executable: impl Into<PathBuf>,
    ) -> Result<CommandSpec> {
        let mut command = self.command_spec()?;
        command.executable = executable.into();
        Ok(command)
    }
}

impl QueryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issues => "issues",
            Self::Pulls => "pulls",
            Self::FailedRuns => "failed_runs",
            Self::Checks => "checks",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhSnapshot {
    pub repo: String,
    pub kind: QueryKind,
    pub status: String,
    pub items: Vec<Value>,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhWatchObservation {
    pub watch_key: String,
    pub repo: String,
    pub kind: QueryKind,
    pub pull_number: Option<u64>,
    pub fingerprint: String,
    pub item_count: usize,
}

pub fn observe(query: &GhQuery, snapshot: &GhSnapshot) -> Result<GhWatchObservation> {
    if snapshot.repo != query.repo || snapshot.kind != query.kind {
        anyhow::bail!("GitHub snapshot does not match the requested query");
    }
    let mut items = snapshot.items.clone();
    items.sort_by_key(|item| serde_json::to_string(item).unwrap_or_default());
    let payload = serde_json::json!({
        "repo": snapshot.repo,
        "kind": snapshot.kind,
        "pull_number": query.pull_number,
        "items": items,
    });
    Ok(GhWatchObservation {
        watch_key: query.watch_key(),
        repo: query.repo.clone(),
        kind: query.kind,
        pull_number: query.pull_number,
        fingerprint: fingerprint_bytes(&serde_json::to_vec(&payload)?),
        item_count: snapshot.items.len(),
    })
}

pub async fn execute(query: &GhQuery, timeout_seconds: u64) -> Result<GhSnapshot> {
    execute_with_executable(query, Path::new("gh"), timeout_seconds).await
}

pub async fn execute_with_executable(
    query: &GhQuery,
    executable: &Path,
    timeout_seconds: u64,
) -> Result<GhSnapshot> {
    let command = query.command_spec_with_executable(executable)?;
    let result = executor::execute(&command, timeout_seconds).await?;
    if result.status != "succeeded" {
        anyhow::bail!("gh query failed: {}", result.stderr.trim());
    }
    let items: Vec<Value> = serde_json::from_str(&result.stdout)
        .with_context(|| format!("parse structured gh output for {:?}", query.kind))?;
    Ok(GhSnapshot {
        repo: query.repo.clone(),
        kind: query.kind,
        status: "succeeded".into(),
        items,
        stderr: result.stderr,
    })
}

fn validate_repo(repo: &str) -> Result<()> {
    if repo.trim().is_empty()
        || repo.starts_with('-')
        || repo.contains(char::is_whitespace)
        || !repo.contains('/')
    {
        anyhow::bail!("repository must be an owner/name value");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    use tempfile::tempdir;

    #[test]
    fn builds_read_only_structured_issue_query() {
        let command = GhQuery {
            repo: "owner/repo".into(),
            kind: QueryKind::Issues,
            pull_number: None,
        }
        .command_spec()
        .unwrap();
        assert_eq!(command.executable, std::path::PathBuf::from("gh"));
        assert!(command.args.contains(&"--json".into()));
        assert!(
            command
                .args
                .contains(&"number,title,labels,createdAt,updatedAt,url".into())
        );
        assert!(
            !command
                .args
                .iter()
                .any(|arg| matches!(arg.as_str(), "comment" | "create" | "edit" | "merge"))
        );
    }

    #[test]
    fn checks_require_a_pull_number_and_repo_validation_is_fail_closed() {
        assert!(
            GhQuery {
                repo: "owner/repo".into(),
                kind: QueryKind::Checks,
                pull_number: None
            }
            .command_spec()
            .is_err()
        );
        assert!(
            GhQuery {
                repo: "--repo".into(),
                kind: QueryKind::Issues,
                pull_number: None
            }
            .command_spec()
            .is_err()
        );
        assert!(
            GhQuery {
                repo: "owner/repo with-space".into(),
                kind: QueryKind::Issues,
                pull_number: None
            }
            .command_spec()
            .is_err()
        );
    }

    #[test]
    fn snapshot_observation_is_order_insensitive_but_content_sensitive() {
        let query = GhQuery {
            repo: "owner/repo".into(),
            kind: QueryKind::Pulls,
            pull_number: None,
        };
        let first = GhSnapshot {
            repo: "owner/repo".into(),
            kind: QueryKind::Pulls,
            status: "succeeded".into(),
            items: vec![
                serde_json::json!({"number": 2}),
                serde_json::json!({"number": 1}),
            ],
            stderr: String::new(),
        };
        let reordered = GhSnapshot {
            items: vec![
                serde_json::json!({"number": 1}),
                serde_json::json!({"number": 2}),
            ],
            ..first.clone()
        };
        let changed = GhSnapshot {
            items: vec![serde_json::json!({"number": 3})],
            ..first.clone()
        };
        assert_eq!(
            observe(&query, &first).unwrap().fingerprint,
            observe(&query, &reordered).unwrap().fingerprint
        );
        assert_ne!(
            observe(&query, &first).unwrap().fingerprint,
            observe(&query, &changed).unwrap().fingerprint
        );
    }

    #[tokio::test]
    async fn executes_structured_output_through_a_fake_gh_binary() {
        let directory = tempdir().unwrap();
        let fake_gh = directory.path().join("gh");
        fs::write(
            &fake_gh,
            "#!/bin/sh\nprintf '%s\\n' '[{\"number\":42,\"title\":\"safe\"}]'\n",
        )
        .unwrap();
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o700)).unwrap();
        let query = GhQuery {
            repo: "owner/repo".into(),
            kind: QueryKind::Pulls,
            pull_number: None,
        };
        let snapshot = execute_with_executable(&query, &fake_gh, 5).await.unwrap();
        assert_eq!(snapshot.items[0]["number"], 42);
        assert_eq!(snapshot.items[0]["title"], "safe");
    }
}
