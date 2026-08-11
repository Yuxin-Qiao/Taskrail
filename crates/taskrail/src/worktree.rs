use crate::codex::ensure_git_repository;
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
pub struct WorktreeHandle {
    pub repository: PathBuf,
    pub path: PathBuf,
}

pub fn create(
    repository: impl AsRef<Path>,
    path: impl AsRef<Path>,
    base: Option<&str>,
) -> Result<WorktreeHandle> {
    let repository = repository
        .as_ref()
        .canonicalize()
        .with_context(|| format!("resolve repository {}", repository.as_ref().display()))?;
    let path = path.as_ref().to_path_buf();
    ensure_git_repository(&repository)?;
    if path.exists() {
        anyhow::bail!("worktree path already exists: {}", path.display());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create worktree parent {}", parent.display()))?;
    }
    let base = base.unwrap_or("HEAD");
    let status = Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args(["worktree", "add", "--detach"])
        .arg(&path)
        .arg(base)
        .status()
        .context("create Git worktree")?;
    if !status.success() {
        anyhow::bail!("git worktree add failed for {}", path.display());
    }
    Ok(WorktreeHandle { repository, path })
}

pub fn is_clean(path: impl AsRef<Path>) -> Result<bool> {
    let path = path.as_ref();
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .context("inspect worktree status")?;
    if !output.status.success() {
        anyhow::bail!("git status failed for {}", path.display());
    }
    Ok(output.stdout.is_empty())
}

pub fn remove(handle: &WorktreeHandle, force: bool) -> Result<()> {
    if !force && !is_clean(&handle.path)? {
        anyhow::bail!(
            "refusing to remove dirty worktree {}; inspect or pass force explicitly",
            handle.path.display()
        );
    }
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(&handle.repository)
        .args(["worktree", "remove"]);
    if force {
        command.arg("--force");
    }
    let status = command
        .arg(&handle.path)
        .status()
        .context("remove Git worktree")?;
    if !status.success() {
        anyhow::bail!("git worktree remove failed for {}", handle.path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn run(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }

    #[test]
    fn creates_and_removes_isolated_worktree() {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        run(&repo, &["init", "-q"]);
        run(&repo, &["config", "user.email", "auto@example.invalid"]);
        run(&repo, &["config", "user.name", "auto-test"]);
        fs::write(repo.join("README"), "test\n").unwrap();
        run(&repo, &["add", "README"]);
        run(&repo, &["commit", "-qm", "initial"]);
        let path = root.path().join("worktree");
        let handle = create(&repo, &path, None).unwrap();
        assert!(handle.path.join("README").exists());
        assert!(is_clean(&handle.path).unwrap());
        remove(&handle, false).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn refuses_dirty_cleanup_without_force() {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        run(&repo, &["init", "-q"]);
        run(&repo, &["config", "user.email", "auto@example.invalid"]);
        run(&repo, &["config", "user.name", "auto-test"]);
        fs::write(repo.join("README"), "test\n").unwrap();
        run(&repo, &["add", "README"]);
        run(&repo, &["commit", "-qm", "initial"]);
        let path = root.path().join("worktree");
        let handle = create(&repo, &path, None).unwrap();
        fs::write(path.join("README"), "changed\n").unwrap();
        assert!(remove(&handle, false).is_err());
        remove(&handle, true).unwrap();
    }
}
