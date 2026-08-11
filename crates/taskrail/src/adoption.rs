use crate::{
    core::{
        AdoptionState, Automation, DiscoveredSource, Ownership, RuntimeState, fingerprint_bytes,
    },
    storage::Registry,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    path::{Path, PathBuf},
    process::Output,
};
use uuid::Uuid;

pub trait NativeController {
    type Snapshot: Serialize + DeserializeOwned + Clone;

    fn snapshot(&self, source: &DiscoveredSource) -> Result<Self::Snapshot>;
    fn disable(&self, snapshot: &Self::Snapshot) -> Result<()>;
    fn verify_disabled(&self, snapshot: &Self::Snapshot) -> Result<bool>;
    fn restore(&self, snapshot: &Self::Snapshot) -> Result<()>;
}

/// Restore a journaled adoption and converge its Registry owner to a
/// fail-closed observed/needs-attention state.
///
/// This is deliberately explicit: callers must choose the transaction to
/// restore. It is also usable for interrupted non-terminal transactions after
/// a daemon restart.
pub fn rollback_adoption<C: NativeController>(
    registry: &Registry,
    tx_id: &str,
    source_id: &str,
    snapshot: &C::Snapshot,
    automation: Option<Automation>,
    controller: &C,
) -> Result<()> {
    registry.update_adoption(tx_id, AdoptionState::RollingBack, "manual_rollback", None)?;
    let restore = controller.restore(snapshot);
    let mut reverted = automation;
    if let Some(automation) = &mut reverted {
        automation.ownership = Ownership::Observed;
        automation.runtime_state = RuntimeState::NeedsAttention;
    }
    if let Some(automation) = &reverted {
        registry.save_automation(automation)?;
    }
    match restore {
        Ok(()) => {
            registry.update_adoption(
                tx_id,
                AdoptionState::RolledBack,
                "manual_rollback_complete",
                None,
            )?;
            registry.append_event(&crate::core::Event {
                run_id: None,
                occurred_at: chrono::Utc::now(),
                event_type: "adoption.rolled_back".into(),
                payload: serde_json::json!({
                    "tx_id": tx_id,
                    "source_id": source_id,
                    "reason": "explicit_rollback",
                }),
            })?;
            Ok(())
        }
        Err(error) => {
            registry.update_adoption(
                tx_id,
                AdoptionState::NeedsAttention,
                "manual_rollback_failed",
                Some(&error.to_string()),
            )?;
            registry.append_event(&crate::core::Event {
                run_id: None,
                occurred_at: chrono::Utc::now(),
                event_type: "adoption.rollback_failed".into(),
                payload: serde_json::json!({
                    "tx_id": tx_id,
                    "source_id": source_id,
                    "error": error.to_string(),
                }),
            })?;
            Err(error).with_context(|| format!("restore adoption transaction {tx_id}"))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSnapshot {
    pub source_id: String,
    pub before: String,
    pub matched_line: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Default)]
pub struct CronController {
    pub current: Option<String>,
}

impl CronController {
    fn current_crontab(&self) -> Result<String> {
        if let Some(current) = &self.current {
            return Ok(current.clone());
        }
        let output = std::process::Command::new("crontab")
            .arg("-l")
            .output()
            .context("run crontab -l")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("no crontab") {
                return Ok(String::new());
            }
            anyhow::bail!("crontab -l failed: {}", stderr.trim());
        }
        String::from_utf8(output.stdout).context("crontab output is not UTF-8")
    }

    fn install_crontab(&self, value: &str) -> Result<()> {
        if self.current.is_some() {
            return Ok(());
        }
        let mut child = std::process::Command::new("crontab")
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("start crontab -")?;
        use std::io::Write;
        child
            .stdin
            .take()
            .context("open crontab stdin")?
            .write_all(value.as_bytes())
            .context("write crontab")?;
        let status = child.wait().context("wait for crontab")?;
        if !status.success() {
            anyhow::bail!("crontab - rejected the replacement");
        }
        Ok(())
    }
}

impl NativeController for CronController {
    type Snapshot = CronSnapshot;

    fn snapshot(&self, source: &DiscoveredSource) -> Result<Self::Snapshot> {
        if source.provider != "cron" {
            anyhow::bail!("cron controller cannot adopt {} source", source.provider);
        }
        let before = self.current_crontab()?;
        let line_number = source
            .native_id
            .strip_prefix("line-")
            .and_then(|value| value.parse::<usize>().ok())
            .context("invalid cron source line identity")?;
        let Some(matched_line) = before
            .lines()
            .nth(line_number.saturating_sub(1))
            .map(str::to_owned)
        else {
            anyhow::bail!(
                "source {} is no longer present in the native crontab",
                source.source_id
            );
        };
        if matched_line.trim() != source.raw.trim() {
            anyhow::bail!(
                "native source changed before adoption; line {} no longer matches the scanned snapshot",
                line_number
            );
        }
        let fingerprint = fingerprint_bytes(matched_line.trim().as_bytes());
        if fingerprint != source.fingerprint {
            anyhow::bail!(
                "native source changed before adoption: expected {}, found {}",
                source.fingerprint,
                fingerprint
            );
        }
        Ok(CronSnapshot {
            source_id: source.source_id.clone(),
            before,
            matched_line,
            fingerprint,
        })
    }

    fn disable(&self, snapshot: &Self::Snapshot) -> Result<()> {
        let marker = format!(
            "# taskrail-adopted {}: {}",
            snapshot.source_id, snapshot.matched_line
        );
        let replacement = snapshot
            .before
            .lines()
            .map(|line| {
                if line == snapshot.matched_line {
                    marker.as_str()
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + if snapshot.before.ends_with('\n') {
                "\n"
            } else {
                ""
            };
        self.install_crontab(&replacement)
    }

    fn verify_disabled(&self, snapshot: &Self::Snapshot) -> Result<bool> {
        let current = self.current_crontab()?;
        Ok(!current
            .lines()
            .any(|line| line.trim() == snapshot.matched_line.trim()))
    }

    fn restore(&self, snapshot: &Self::Snapshot) -> Result<()> {
        self.install_crontab(&snapshot.before)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchdSnapshot {
    pub source_id: String,
    pub label: String,
    pub path: PathBuf,
    pub before: Vec<u8>,
    pub fingerprint: String,
    pub domain: String,
    pub was_loaded: bool,
}

#[derive(Debug, Clone)]
pub struct LaunchdController {
    pub launchctl_path: PathBuf,
    /// Test and embedding override; production resolves the current user's UID.
    pub user_domain: Option<String>,
    /// Test and embedding override; production uses `$HOME`.
    pub home: Option<PathBuf>,
}

impl Default for LaunchdController {
    fn default() -> Self {
        Self {
            launchctl_path: PathBuf::from("launchctl"),
            user_domain: None,
            home: None,
        }
    }
}

impl LaunchdController {
    fn run(&self, args: &[&str]) -> Result<Output> {
        std::process::Command::new(&self.launchctl_path)
            .args(args)
            .output()
            .with_context(|| format!("run {} {}", self.launchctl_path.display(), args.join(" ")))
    }

    fn home(&self) -> Result<PathBuf> {
        self.home
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .context("resolve HOME for launchd adoption")
    }

    fn domain(&self) -> Result<String> {
        if let Some(domain) = &self.user_domain {
            return Ok(domain.clone());
        }
        let output = std::process::Command::new("id")
            .arg("-u")
            .output()
            .context("resolve current user UID for launchd")?;
        if !output.status.success() {
            anyhow::bail!(
                "id -u failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(format!(
            "gui/{}",
            String::from_utf8(output.stdout)
                .context("current user UID is not UTF-8")?
                .trim()
        ))
    }

    fn validate_user_agent_path(&self, path: &Path) -> Result<PathBuf> {
        let root = self
            .home()?
            .join("Library")
            .join("LaunchAgents")
            .canonicalize()
            .with_context(|| format!("resolve user LaunchAgents root for {}", path.display()))?;
        let path = path
            .canonicalize()
            .with_context(|| format!("resolve launchd plist {}", path.display()))?;
        if !path.starts_with(&root)
            || path.extension().and_then(|value| value.to_str()) != Some("plist")
        {
            anyhow::bail!(
                "launchd adoption is limited to the current user's Library/LaunchAgents plist"
            );
        }
        Ok(path)
    }

    fn target(&self, domain: &str, label: &str) -> String {
        format!("{domain}/{label}")
    }
}

impl NativeController for LaunchdController {
    type Snapshot = LaunchdSnapshot;

    fn snapshot(&self, source: &DiscoveredSource) -> Result<Self::Snapshot> {
        if source.provider != "launchd" {
            anyhow::bail!("launchd controller cannot adopt {} source", source.provider);
        }
        let source_path = source
            .path
            .as_deref()
            .context("launchd source has no plist path")?;
        let path = self.validate_user_agent_path(source_path)?;
        let before = std::fs::read(&path)
            .with_context(|| format!("read launchd plist {}", path.display()))?;
        let current = crate::discovery::parse_launchd_plist(&path)?
            .context("launchd plist is not a dictionary")?;
        if current.native_id != source.native_id {
            anyhow::bail!(
                "launchd label changed before adoption: expected {}, found {}",
                source.native_id,
                current.native_id
            );
        }
        let fingerprint = fingerprint_bytes(&before);
        if fingerprint != source.fingerprint {
            anyhow::bail!(
                "launchd plist changed before adoption: expected {}, found {}",
                source.fingerprint,
                fingerprint
            );
        }
        let domain = self.domain()?;
        let target = self.target(&domain, &source.native_id);
        let loaded = self.run(&["print", &target])?.status.success();
        Ok(LaunchdSnapshot {
            source_id: source.source_id.clone(),
            label: source.native_id.clone(),
            path,
            before,
            fingerprint,
            domain,
            was_loaded: loaded,
        })
    }

    fn disable(&self, snapshot: &Self::Snapshot) -> Result<()> {
        if !snapshot.was_loaded {
            return Ok(());
        }
        let target = self.target(&snapshot.domain, &snapshot.label);
        let output = self.run(&["bootout", &target])?;
        if !output.status.success() {
            anyhow::bail!(
                "launchctl bootout {} failed: {}",
                target,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    fn verify_disabled(&self, snapshot: &Self::Snapshot) -> Result<bool> {
        let target = self.target(&snapshot.domain, &snapshot.label);
        Ok(!self.run(&["print", &target])?.status.success())
    }

    fn restore(&self, snapshot: &Self::Snapshot) -> Result<()> {
        if !snapshot.was_loaded {
            return Ok(());
        }
        let path = snapshot.path.to_string_lossy().into_owned();
        let output = self.run(&["bootstrap", &snapshot.domain, &path])?;
        if !output.status.success() {
            anyhow::bail!(
                "launchctl bootstrap {} {} failed: {}",
                snapshot.domain,
                path,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdSnapshot {
    pub source_id: String,
    pub unit: String,
    pub before: String,
    pub fingerprint: String,
    pub was_enabled: bool,
    pub was_active: bool,
}

#[derive(Debug, Clone)]
pub struct SystemdController {
    pub systemctl_path: PathBuf,
}

impl Default for SystemdController {
    fn default() -> Self {
        Self {
            systemctl_path: PathBuf::from("systemctl"),
        }
    }
}

impl SystemdController {
    fn run(&self, args: &[&str]) -> Result<std::process::Output> {
        std::process::Command::new(&self.systemctl_path)
            .args(args)
            .output()
            .with_context(|| format!("run {} {}", self.systemctl_path.display(), args.join(" ")))
    }

    fn show(&self, unit: &str) -> Result<String> {
        let output = self.run(&[
            "--user",
            "show",
            unit,
            "--no-pager",
            "--property=FragmentPath,ExecStart,OnUnitActiveSec,OnCalendar,UnitFileState,ActiveState",
        ])?;
        if !output.status.success() {
            anyhow::bail!(
                "systemctl --user show {unit} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout).context("systemctl show output is not UTF-8")
    }

    fn state<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
        raw.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
    }
}

impl NativeController for SystemdController {
    type Snapshot = SystemdSnapshot;

    fn snapshot(&self, source: &DiscoveredSource) -> Result<Self::Snapshot> {
        if source.provider != "systemd" {
            anyhow::bail!("systemd controller cannot adopt {} source", source.provider);
        }
        if !source.native_id.ends_with(".service") {
            anyhow::bail!("systemd adoption requires a .service unit");
        }
        let before = self.show(&source.native_id)?;
        let fingerprint = fingerprint_bytes(before.as_bytes());
        if fingerprint != source.fingerprint {
            anyhow::bail!(
                "systemd unit changed before adoption: expected {}, found {}",
                source.fingerprint,
                fingerprint
            );
        }
        let unit_state = Self::state(&before, "UnitFileState").unwrap_or("unknown");
        if unit_state != "enabled" {
            anyhow::bail!(
                "systemd unit {} is not explicitly enabled (state: {unit_state})",
                source.native_id
            );
        }
        Ok(SystemdSnapshot {
            source_id: source.source_id.clone(),
            unit: source.native_id.clone(),
            before,
            fingerprint,
            was_enabled: true,
            was_active: matches!(
                Self::state(source.raw.as_str(), "ActiveState"),
                Some("active" | "activating" | "reloading")
            ),
        })
    }

    fn disable(&self, snapshot: &Self::Snapshot) -> Result<()> {
        let output = self.run(&["--user", "disable", "--now", &snapshot.unit])?;
        if !output.status.success() {
            anyhow::bail!(
                "systemctl --user disable --now {} failed: {}",
                snapshot.unit,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    fn verify_disabled(&self, snapshot: &Self::Snapshot) -> Result<bool> {
        let enabled = self.run(&["--user", "is-enabled", &snapshot.unit])?;
        let active = self.run(&["--user", "is-active", &snapshot.unit])?;
        let enabled_state = String::from_utf8_lossy(&enabled.stdout).trim().to_owned();
        let active_state = String::from_utf8_lossy(&active.stdout).trim().to_owned();
        Ok(matches!(
            enabled_state.as_str(),
            "disabled" | "indirect" | "masked" | "not-found"
        ) && matches!(
            active_state.as_str(),
            "inactive" | "dead" | "failed" | "unknown" | "not-found"
        ))
    }

    fn restore(&self, snapshot: &Self::Snapshot) -> Result<()> {
        let action = if snapshot.was_enabled {
            "enable"
        } else {
            "disable"
        };
        let now = snapshot.was_active.then_some("--now");
        let mut args = vec!["--user", action];
        if let Some(now) = now {
            args.push(now);
        }
        args.push(&snapshot.unit);
        let output = self.run(&args)?;
        if !output.status.success() {
            anyhow::bail!(
                "systemctl --user {} {} failed: {}",
                action,
                snapshot.unit,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AdoptionReport {
    pub tx_id: String,
    pub source_id: String,
    pub state: AdoptionState,
    pub dry_run: bool,
    pub message: String,
}

pub struct AdoptionEngine<'a, C> {
    registry: &'a Registry,
    controller: C,
}

impl<'a, C> AdoptionEngine<'a, C>
where
    C: NativeController,
{
    pub fn new(registry: &'a Registry, controller: C) -> Self {
        Self {
            registry,
            controller,
        }
    }

    pub fn adopt(
        &self,
        source: &DiscoveredSource,
        mut automation: Automation,
        apply: bool,
    ) -> Result<AdoptionReport> {
        if automation.ownership != Ownership::Observed {
            anyhow::bail!(
                "source {} already has {:?} ownership; adoption requires an observed automation",
                source.source_id,
                automation.ownership
            );
        }
        if automation.runtime_state == RuntimeState::NeedsAttention {
            anyhow::bail!(
                "source {} needs attention before adoption",
                source.source_id
            );
        }
        if source
            .command
            .as_ref()
            .is_some_and(|command| command.shell || command.invokes_shell())
        {
            anyhow::bail!(
                "native source {} invokes a shell; adoption is refused until it is converted to direct argv",
                source.source_id
            );
        }
        if source.provider == "launchd"
            && source
                .path
                .as_ref()
                .is_some_and(|path| path.starts_with("/Library/LaunchDaemons"))
        {
            anyhow::bail!("system LaunchDaemons are observation-only");
        }
        let snapshot = self.controller.snapshot(source)?;
        if snapshot_fingerprint(&snapshot)? != source.fingerprint {
            anyhow::bail!("source fingerprint changed during preflight; adoption aborted");
        }
        // The source must exist before the staged automation can reference it.
        // This is idempotent and keeps the transaction valid for API callers that
        // discovered a source without first persisting a scan result.
        self.registry.upsert_source(source)?;
        let tx_id = format!("adopt_{}", Uuid::new_v4());
        if !apply {
            return Ok(AdoptionReport {
                tx_id,
                source_id: source.source_id.clone(),
                state: AdoptionState::Preparing,
                dry_run: true,
                message: "preflight passed; no native source changed".into(),
            });
        }
        let snapshot_json = serde_json::to_string(&snapshot)?;
        self.registry
            .begin_adoption(&tx_id, &source.source_id, &snapshot_json)?;

        automation.ownership = Ownership::Adopted;
        automation.runtime_state = RuntimeState::Paused;
        automation.source_id = Some(source.source_id.clone());
        automation.fingerprint = Some(source.fingerprint.clone());
        self.registry.save_automation(&automation)?;

        if let Err(error) = self.controller.disable(&snapshot).and_then(|_| {
            self.registry.update_adoption(
                &tx_id,
                AdoptionState::NativeDisabled,
                "disable_native",
                None,
            )?;
            if !self.controller.verify_disabled(&snapshot)? {
                anyhow::bail!("native source still appears active after disable");
            }
            Ok(())
        }) {
            return self.rollback_after_failure(&tx_id, &snapshot, &automation, error);
        }

        automation.runtime_state = RuntimeState::Enabled;
        self.registry.save_automation(&automation)?;
        self.registry.update_adoption(
            &tx_id,
            AdoptionState::InternalEnabled,
            "enable_internal",
            None,
        )?;
        if self
            .registry
            .count_owners_for_fingerprint(&source.fingerprint)?
            != 1
        {
            return self.rollback_after_failure(
                &tx_id,
                &snapshot,
                &automation,
                anyhow::anyhow!("ownership proof failed: expected exactly one owner"),
            );
        }
        self.registry
            .update_adoption(&tx_id, AdoptionState::Committed, "commit", None)?;
        Ok(AdoptionReport {
            tx_id,
            source_id: source.source_id.clone(),
            state: AdoptionState::Committed,
            dry_run: false,
            message: "native source disabled and internal owner committed".into(),
        })
    }

    fn rollback_after_failure(
        &self,
        tx_id: &str,
        snapshot: &C::Snapshot,
        automation: &Automation,
        error: anyhow::Error,
    ) -> Result<AdoptionReport> {
        self.registry.update_adoption(
            tx_id,
            AdoptionState::RollingBack,
            "rollback",
            Some(&error.to_string()),
        )?;
        let restore = self.controller.restore(snapshot);
        let mut reverted = automation.clone();
        reverted.ownership = Ownership::Observed;
        reverted.runtime_state = RuntimeState::NeedsAttention;
        self.registry.save_automation(&reverted)?;
        match restore {
            Ok(()) => {
                self.registry.update_adoption(
                    tx_id,
                    AdoptionState::RolledBack,
                    "rollback_complete",
                    Some(&error.to_string()),
                )?;
                Ok(AdoptionReport {
                    tx_id: tx_id.into(),
                    source_id: automation.source_id.clone().unwrap_or_default(),
                    state: AdoptionState::RolledBack,
                    dry_run: false,
                    message: format!("adoption rolled back: {error}"),
                })
            }
            Err(restore_error) => {
                self.registry.update_adoption(
                    tx_id,
                    AdoptionState::NeedsAttention,
                    "rollback_failed",
                    Some(&format!("{error}; restore failed: {restore_error}")),
                )?;
                anyhow::bail!(
                    "adoption failed and rollback also failed: {error}; restore failed: {restore_error}"
                )
            }
        }
    }
}

fn snapshot_fingerprint<T: Serialize>(snapshot: &T) -> Result<String> {
    let value = serde_json::to_value(snapshot)?;
    Ok(value
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CommandSpec, Trigger};
    use std::{cell::RefCell, fs, os::unix::fs::PermissionsExt, rc::Rc};
    use tempfile::tempdir;

    #[derive(Clone)]
    struct FakeController {
        active: Rc<RefCell<bool>>,
        fail_disable: bool,
        fail_verify: bool,
        fail_restore: bool,
    }

    #[derive(Clone, Serialize, Deserialize)]
    struct FakeSnapshot {
        fingerprint: String,
    }

    impl NativeController for FakeController {
        type Snapshot = FakeSnapshot;
        fn snapshot(&self, source: &DiscoveredSource) -> Result<Self::Snapshot> {
            Ok(FakeSnapshot {
                fingerprint: source.fingerprint.clone(),
            })
        }
        fn disable(&self, _snapshot: &Self::Snapshot) -> Result<()> {
            if self.fail_disable {
                anyhow::bail!("injected disable failure");
            }
            *self.active.borrow_mut() = false;
            Ok(())
        }
        fn verify_disabled(&self, _snapshot: &Self::Snapshot) -> Result<bool> {
            if self.fail_verify {
                anyhow::bail!("injected verification failure");
            }
            Ok(!*self.active.borrow())
        }
        fn restore(&self, _snapshot: &Self::Snapshot) -> Result<()> {
            if self.fail_restore {
                anyhow::bail!("injected restore failure");
            }
            *self.active.borrow_mut() = true;
            Ok(())
        }
    }

    fn source() -> DiscoveredSource {
        DiscoveredSource {
            source_id: "fake:1".into(),
            provider: "cron".into(),
            native_id: "fake".into(),
            path: None,
            enabled: true,
            kind: "task".into(),
            fingerprint: "sha256:f".into(),
            command: Some(CommandSpec::argv("echo", ["ok"])),
            trigger: Trigger::Manual,
            raw: "echo".into(),
        }
    }

    #[test]
    fn dry_run_does_not_change_native_owner() {
        let registry = Registry::in_memory().unwrap();
        let active = Rc::new(RefCell::new(true));
        let engine = AdoptionEngine::new(
            &registry,
            FakeController {
                active: active.clone(),
                fail_disable: false,
                fail_verify: false,
                fail_restore: false,
            },
        );
        let report = engine
            .adopt(&source(), Automation::default(), false)
            .unwrap();
        assert!(report.dry_run);
        assert!(*active.borrow());
        assert!(registry.list_automations().unwrap().is_empty());
    }

    #[test]
    fn commit_requires_one_owner_and_records_journal() {
        let registry = Registry::in_memory().unwrap();
        let active = Rc::new(RefCell::new(true));
        let engine = AdoptionEngine::new(
            &registry,
            FakeController {
                active: active.clone(),
                fail_disable: false,
                fail_verify: false,
                fail_restore: false,
            },
        );
        let automation = Automation {
            steps: vec![crate::core::StepSpec {
                id: "main".into(),
                command: CommandSpec::argv("echo", ["ok"]),
                responses: None,
            }],
            ..Automation::default()
        };
        let report = engine.adopt(&source(), automation, true).unwrap();
        assert_eq!(report.state, AdoptionState::Committed);
        assert!(!*active.borrow());
        assert_eq!(
            registry.adoption(&report.tx_id).unwrap().unwrap().1,
            AdoptionState::Committed
        );
        assert_eq!(
            registry.count_owners_for_fingerprint("sha256:f").unwrap(),
            1
        );
    }

    #[test]
    fn failure_restores_native_source() {
        let registry = Registry::in_memory().unwrap();
        let active = Rc::new(RefCell::new(true));
        let engine = AdoptionEngine::new(
            &registry,
            FakeController {
                active: active.clone(),
                fail_disable: true,
                fail_verify: false,
                fail_restore: false,
            },
        );
        let report = engine
            .adopt(&source(), Automation::default(), true)
            .unwrap();
        assert_eq!(report.state, AdoptionState::RolledBack);
        assert!(*active.borrow());
    }

    #[test]
    fn verification_failure_restores_native_source_and_marks_attention() {
        let registry = Registry::in_memory().unwrap();
        let active = Rc::new(RefCell::new(true));
        let engine = AdoptionEngine::new(
            &registry,
            FakeController {
                active: active.clone(),
                fail_disable: false,
                fail_verify: true,
                fail_restore: false,
            },
        );
        let automation = Automation {
            id: "verify-failure".into(),
            ..Automation::default()
        };
        let report = engine.adopt(&source(), automation, true).unwrap();
        assert_eq!(report.state, AdoptionState::RolledBack);
        assert!(*active.borrow());
        let stored = registry.get_automation("verify-failure").unwrap().unwrap();
        assert_eq!(stored.ownership, Ownership::Observed);
        assert_eq!(stored.runtime_state, RuntimeState::NeedsAttention);
    }

    #[test]
    fn restore_failure_stops_with_needs_attention_and_no_false_commit() {
        let registry = Registry::in_memory().unwrap();
        let active = Rc::new(RefCell::new(true));
        let engine = AdoptionEngine::new(
            &registry,
            FakeController {
                active: active.clone(),
                fail_disable: false,
                fail_verify: true,
                fail_restore: true,
            },
        );
        let automation = Automation {
            id: "restore-failure".into(),
            ..Automation::default()
        };
        let error = engine.adopt(&source(), automation, true).unwrap_err();
        assert!(error.to_string().contains("rollback also failed"));
        assert!(!*active.borrow());
        let stored = registry.get_automation("restore-failure").unwrap().unwrap();
        assert_eq!(stored.ownership, Ownership::Observed);
        assert_eq!(stored.runtime_state, RuntimeState::NeedsAttention);
        let adoption = registry.list_adoptions(1).unwrap().pop().unwrap();
        assert_eq!(adoption.state, AdoptionState::NeedsAttention);
    }

    #[test]
    fn duplicate_owner_proof_rolls_back_without_leaving_two_owners() {
        let registry = Registry::in_memory().unwrap();
        let active = Rc::new(RefCell::new(true));
        let mut existing = Automation {
            id: "existing-owner".into(),
            ownership: Ownership::Managed,
            fingerprint: Some(source().fingerprint),
            ..Automation::default()
        };
        existing.name = "existing-owner".into();
        registry.save_automation(&existing).unwrap();
        let engine = AdoptionEngine::new(
            &registry,
            FakeController {
                active: active.clone(),
                fail_disable: false,
                fail_verify: false,
                fail_restore: false,
            },
        );
        let automation = Automation {
            id: "duplicate-owner".into(),
            ..Automation::default()
        };
        let report = engine.adopt(&source(), automation, true).unwrap();
        assert_eq!(report.state, AdoptionState::RolledBack);
        assert!(*active.borrow());
        assert_eq!(
            registry.count_owners_for_fingerprint("sha256:f").unwrap(),
            1
        );
        let stored = registry.get_automation("duplicate-owner").unwrap().unwrap();
        assert_eq!(stored.ownership, Ownership::Observed);
        assert_eq!(stored.runtime_state, RuntimeState::NeedsAttention);
    }

    #[test]
    fn explicit_rollback_restores_native_and_converges_registry_owner() {
        let registry = Registry::in_memory().unwrap();
        let active = Rc::new(RefCell::new(false));
        let controller = FakeController {
            active: active.clone(),
            fail_disable: false,
            fail_verify: false,
            fail_restore: false,
        };
        let source = source();
        registry.upsert_source(&source).unwrap();
        let automation = Automation {
            id: "manual-rollback".into(),
            name: "manual-rollback".into(),
            ownership: Ownership::Adopted,
            runtime_state: RuntimeState::Enabled,
            source_id: Some(source.source_id.clone()),
            fingerprint: Some(source.fingerprint.clone()),
            ..Automation::default()
        };
        registry.save_automation(&automation).unwrap();
        let snapshot = FakeSnapshot {
            fingerprint: source.fingerprint.clone(),
        };
        registry
            .begin_adoption(
                "tx-manual",
                &source.source_id,
                &serde_json::to_string(&snapshot).unwrap(),
            )
            .unwrap();
        rollback_adoption(
            &registry,
            "tx-manual",
            &source.source_id,
            &snapshot,
            Some(automation),
            &controller,
        )
        .unwrap();
        assert!(*active.borrow());
        let stored = registry.get_automation("manual-rollback").unwrap().unwrap();
        assert_eq!(stored.ownership, Ownership::Observed);
        assert_eq!(stored.runtime_state, RuntimeState::NeedsAttention);
        assert_eq!(
            registry.get_adoption("tx-manual").unwrap().unwrap().state,
            AdoptionState::RolledBack
        );
    }

    #[test]
    fn adoption_refuses_shell_invoking_native_source_before_disable() {
        let registry = Registry::in_memory().unwrap();
        let active = Rc::new(RefCell::new(true));
        let mut source = source();
        source.command = Some(CommandSpec::argv("/bin/sh", ["-c", "echo unsafe"]));
        let engine = AdoptionEngine::new(
            &registry,
            FakeController {
                active: active.clone(),
                fail_disable: false,
                fail_verify: false,
                fail_restore: false,
            },
        );
        assert!(engine.adopt(&source, Automation::default(), true).is_err());
        assert!(*active.borrow());
    }

    #[test]
    fn systemd_controller_disables_and_restores_a_user_service() {
        let directory = tempdir().unwrap();
        let state_file = directory.path().join("state");
        fs::write(&state_file, "enabled\nactive\n").unwrap();
        let raw = "FragmentPath=/home/me/.config/systemd/user/auto.service\nExecStart=/usr/bin/auto --daemon\nOnUnitActiveSec=5min\nUnitFileState=enabled\nActiveState=active\n";
        let fake_systemctl = directory.path().join("systemctl");
        fs::write(
            &fake_systemctl,
            format!(
                r##"#!/bin/sh
state_file='{}'
case "$2" in
  show)
    printf '%b' '{}'
    ;;
  disable)
    printf '%s\n' 'disabled' 'inactive' > "$state_file"
    ;;
  is-enabled)
    sed -n '1p' "$state_file"
    ;;
  is-active)
    sed -n '2p' "$state_file"
    ;;
  enable)
    printf '%s\n' 'enabled' 'active' > "$state_file"
    ;;
esac
"##,
                state_file.display(),
                raw.replace('\n', "\\n")
            ),
        )
        .unwrap();
        fs::set_permissions(&fake_systemctl, fs::Permissions::from_mode(0o700)).unwrap();
        let controller = SystemdController {
            systemctl_path: fake_systemctl,
        };
        let source = DiscoveredSource {
            source_id: "systemd:user:auto.service".into(),
            provider: "systemd".into(),
            native_id: "auto.service".into(),
            path: None,
            enabled: true,
            kind: "service".into(),
            fingerprint: fingerprint_bytes(raw.as_bytes()),
            command: Some(CommandSpec::argv("/usr/bin/auto", ["--daemon"])),
            trigger: Trigger::Interval { seconds: 300 },
            raw: raw.into(),
        };
        let snapshot = controller.snapshot(&source).unwrap();
        assert!(snapshot.was_enabled);
        assert!(snapshot.was_active);
        controller.disable(&snapshot).unwrap();
        assert!(controller.verify_disabled(&snapshot).unwrap());
        controller.restore(&snapshot).unwrap();
        assert!(!controller.verify_disabled(&snapshot).unwrap());
    }

    #[test]
    fn launchd_controller_disables_and_restores_only_a_user_agent() {
        let directory = tempdir().unwrap();
        let agents = directory.path().join("Library/LaunchAgents");
        fs::create_dir_all(&agents).unwrap();
        let plist = agents.join("com.example.auto.plist");
        fs::write(
            &plist,
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>com.example.auto</string><key>ProgramArguments</key><array><string>/bin/echo</string><string>ok</string></array></dict></plist>"#,
        )
        .unwrap();
        let state_file = directory.path().join("loaded");
        fs::write(&state_file, "loaded\n").unwrap();
        let fake_launchctl = directory.path().join("launchctl");
        fs::write(
            &fake_launchctl,
            format!(
                r##"#!/bin/sh
state_file='{}'
case "$1" in
  print)
    test -f "$state_file"
    ;;
  bootout)
    rm -f "$state_file"
    ;;
  bootstrap)
    touch "$state_file"
    ;;
esac
"##,
                state_file.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&fake_launchctl, fs::Permissions::from_mode(0o700)).unwrap();
        let source = crate::discovery::parse_launchd_plist(&plist)
            .unwrap()
            .unwrap();
        let controller = LaunchdController {
            launchctl_path: fake_launchctl,
            user_domain: Some("gui/501".into()),
            home: Some(directory.path().to_path_buf()),
        };
        let snapshot = controller.snapshot(&source).unwrap();
        assert!(snapshot.was_loaded);
        assert!(!snapshot.before.is_empty());
        controller.disable(&snapshot).unwrap();
        assert!(controller.verify_disabled(&snapshot).unwrap());
        controller.restore(&snapshot).unwrap();
        assert!(!controller.verify_disabled(&snapshot).unwrap());

        let mut system_source = source;
        system_source.path = Some(PathBuf::from(
            "/Library/LaunchDaemons/com.example.auto.plist",
        ));
        assert!(controller.snapshot(&system_source).is_err());
    }
}
