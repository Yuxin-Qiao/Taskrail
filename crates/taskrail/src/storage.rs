use crate::core::{
    AdoptionState, Automation, DiscoveredSource, Event, Metric, Ownership, RunResult, RuntimeState,
    canonical_json,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Registry {
    path: PathBuf,
    connection: Connection,
}

pub type AdoptionRecord = (String, AdoptionState, String, String, Option<String>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceReconciliation {
    CreatedObserved,
    UpdatedObserved,
    RetainedOwned,
    Drifted,
    Unrunnable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub seq: i64,
    pub run_id: Option<String>,
    pub occurred_at: String,
    pub event_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRun {
    pub id: String,
    pub automation_id: String,
    pub automation_revision: u64,
    pub automation_snapshot: serde_json::Value,
    pub status: String,
    pub scheduled_at: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRunLogs {
    pub run_id: String,
    pub automation_id: String,
    pub status: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAdoption {
    pub tx_id: String,
    pub source_id: String,
    pub state: AdoptionState,
    pub snapshot: serde_json::Value,
    pub step: String,
    pub last_error: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredApproval {
    pub id: String,
    pub integration: String,
    pub action: String,
    pub plan_fingerprint: String,
    pub plan: serde_json::Value,
    pub request: serde_json::Value,
    pub risk: String,
    pub status: String,
    pub reason: String,
    pub created_at: String,
    pub expires_at: String,
    pub decided_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: String,
    pub kind: String,
    pub severity: String,
    pub status: String,
    pub title: String,
    pub created_at: Option<String>,
    pub detail: serde_json::Value,
}

impl Registry {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create registry directory {}", parent.display()))?;
        }
        let connection =
            Connection::open(&path).with_context(|| format!("open registry {}", path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let registry = Self { path, connection };
        registry.migrate()?;
        Ok(registry)
    }

    pub fn in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let registry = Self {
            path: PathBuf::from(":memory:"),
            connection,
        };
        registry.migrate()?;
        Ok(registry)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> Result<()> {
        self.connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS sources (
               id TEXT PRIMARY KEY,
               provider TEXT NOT NULL,
               native_id TEXT NOT NULL,
               path TEXT,
               enabled INTEGER NOT NULL,
               kind TEXT NOT NULL,
               fingerprint TEXT NOT NULL,
               command_json TEXT,
               trigger_json TEXT NOT NULL,
               raw TEXT NOT NULL,
               observed_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS automations (
               id TEXT PRIMARY KEY,
               name TEXT NOT NULL,
               ownership TEXT NOT NULL,
               runtime_state TEXT NOT NULL,
               definition_json TEXT NOT NULL,
               revision INTEGER NOT NULL,
               source_id TEXT REFERENCES sources(id),
               fingerprint TEXT,
               next_run_at TEXT
             );
             CREATE TABLE IF NOT EXISTS runs (
               id TEXT PRIMARY KEY,
               automation_id TEXT NOT NULL,
               automation_revision INTEGER NOT NULL,
               status TEXT NOT NULL,
               scheduled_at TEXT,
               started_at TEXT NOT NULL,
               ended_at TEXT,
               exit_code INTEGER,
               automation_snapshot_json TEXT NOT NULL DEFAULT '{}',
               stdout TEXT NOT NULL,
               stderr TEXT NOT NULL,
               FOREIGN KEY(automation_id) REFERENCES automations(id)
             );
             CREATE TABLE IF NOT EXISTS events (
               seq INTEGER PRIMARY KEY AUTOINCREMENT,
               run_id TEXT,
               occurred_at TEXT NOT NULL,
               type TEXT NOT NULL,
               payload_json TEXT NOT NULL,
               FOREIGN KEY(run_id) REFERENCES runs(id)
             );
             CREATE TABLE IF NOT EXISTS adoption_journal (
               tx_id TEXT PRIMARY KEY,
               source_id TEXT NOT NULL,
               state TEXT NOT NULL,
               snapshot_json TEXT NOT NULL,
               step TEXT NOT NULL,
               last_error TEXT,
               updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS metrics (
               id TEXT PRIMARY KEY,
               run_id TEXT,
               key TEXT NOT NULL,
               value REAL NOT NULL,
               unit TEXT NOT NULL,
               source TEXT NOT NULL,
               recorded_at TEXT NOT NULL,
               FOREIGN KEY(run_id) REFERENCES runs(id)
             );
             CREATE TABLE IF NOT EXISTS github_snapshots (
               watch_key TEXT PRIMARY KEY,
               repo TEXT NOT NULL,
               kind TEXT NOT NULL,
               pull_number INTEGER,
               fingerprint TEXT NOT NULL,
               snapshot_json TEXT NOT NULL,
               observed_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS approvals (
               id TEXT PRIMARY KEY,
               integration TEXT NOT NULL,
               action TEXT NOT NULL,
               plan_fingerprint TEXT NOT NULL,
               plan_json TEXT NOT NULL,
               request_json TEXT NOT NULL,
               risk TEXT NOT NULL,
               status TEXT NOT NULL,
               reason TEXT NOT NULL,
               created_at TEXT NOT NULL,
               expires_at TEXT NOT NULL,
               decided_at TEXT
             );
             CREATE INDEX IF NOT EXISTS runs_automation_idx ON runs(automation_id, started_at DESC);
             CREATE INDEX IF NOT EXISTS events_run_idx ON events(run_id, seq);
             CREATE INDEX IF NOT EXISTS metrics_recorded_idx ON metrics(recorded_at DESC);
             CREATE INDEX IF NOT EXISTS approvals_status_idx ON approvals(status, expires_at DESC);",
        )?;
        self.ensure_column(
            "runs",
            "automation_snapshot_json",
            "TEXT NOT NULL DEFAULT '{}'",
        )?;
        self.ensure_column("approvals", "plan_json", "TEXT NOT NULL DEFAULT '{}'")?;
        self.ensure_column("approvals", "request_json", "TEXT NOT NULL DEFAULT '{}'")?;
        Ok(())
    }

    fn ensure_column(&self, table: &str, column: &str, definition: &str) -> Result<()> {
        let exists: Option<String> = self
            .connection
            .query_row(
                "SELECT name FROM pragma_table_info(?1) WHERE name = ?2",
                params![table, column],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            self.connection.execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )?;
        }
        Ok(())
    }

    pub fn upsert_source(&self, source: &DiscoveredSource) -> Result<()> {
        self.connection.execute(
            "INSERT INTO sources (id, provider, native_id, path, enabled, kind, fingerprint, command_json, trigger_json, raw, observed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET provider=excluded.provider, native_id=excluded.native_id,
               path=excluded.path, enabled=excluded.enabled, kind=excluded.kind,
               fingerprint=excluded.fingerprint, command_json=excluded.command_json,
               trigger_json=excluded.trigger_json, raw=excluded.raw, observed_at=excluded.observed_at",
            params![
                source.source_id,
                source.provider,
                source.native_id,
                source.path.as_ref().map(|p| p.to_string_lossy().to_string()),
                source.enabled,
                source.kind,
                source.fingerprint,
                source.command.as_ref().map(serde_json::to_string).transpose()?,
                serde_json::to_string(&source.trigger)?,
                source.raw,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn reconcile_discovered_source(
        &self,
        source: &DiscoveredSource,
    ) -> Result<SourceReconciliation> {
        self.upsert_source(source)?;
        let existing = self.get_automation(&source.source_id)?;
        let Some(mut observed) = source.as_observed_automation() else {
            if let Some(mut existing) = existing
                && existing.ownership == crate::core::Ownership::Observed
            {
                let already_needs_attention =
                    existing.runtime_state == crate::core::RuntimeState::NeedsAttention;
                existing.runtime_state = crate::core::RuntimeState::NeedsAttention;
                self.save_automation(&existing)?;
                if !already_needs_attention {
                    self.append_event(&Event {
                        run_id: None,
                        occurred_at: Utc::now(),
                        event_type: "source.unrunnable".into(),
                        payload: serde_json::json!({
                            "source_id": source.source_id,
                            "reason": "shell-invoking or missing direct argv command",
                        }),
                    })?;
                }
                return Ok(SourceReconciliation::Unrunnable);
            }
            return Ok(SourceReconciliation::RetainedOwned);
        };
        let Some(existing) = existing else {
            self.save_automation(&observed)?;
            return Ok(SourceReconciliation::CreatedObserved);
        };
        if existing.ownership == crate::core::Ownership::Observed {
            observed.revision = existing.revision;
            observed.next_run_at = existing.next_run_at;
            self.save_automation(&observed)?;
            return Ok(SourceReconciliation::UpdatedObserved);
        }
        let expected = existing.fingerprint.clone().unwrap_or_default();
        if expected != source.fingerprint {
            let already_needs_attention =
                existing.runtime_state == crate::core::RuntimeState::NeedsAttention;
            let mut drifted = existing.clone();
            drifted.runtime_state = crate::core::RuntimeState::NeedsAttention;
            self.save_automation(&drifted)?;
            if !already_needs_attention {
                self.append_event(&Event {
                    run_id: None,
                    occurred_at: Utc::now(),
                    event_type: "source.drifted".into(),
                    payload: serde_json::json!({
                        "source_id": source.source_id,
                        "ownership": existing.ownership,
                        "expected_fingerprint": expected,
                        "observed_fingerprint": source.fingerprint,
                    }),
                })?;
            }
            return Ok(SourceReconciliation::Drifted);
        }
        Ok(SourceReconciliation::RetainedOwned)
    }

    pub fn acknowledge_source_drift(&self, id_or_native_id: &str) -> Result<Automation> {
        let source = self
            .list_sources()?
            .into_iter()
            .find(|source| {
                source.source_id == id_or_native_id || source.native_id == id_or_native_id
            })
            .with_context(|| format!("source not found: {id_or_native_id}"))?;
        let mut automation = self
            .get_automation(&source.source_id)?
            .with_context(|| format!("source has no Registry automation: {}", source.source_id))?;
        if !matches!(
            automation.ownership,
            Ownership::Adopted | Ownership::Managed
        ) {
            anyhow::bail!(
                "{} is observed-only; drift acknowledgement requires control-plane ownership",
                automation.name
            );
        }
        let expected = automation.fingerprint.clone().unwrap_or_default();
        if expected == source.fingerprint
            || automation.runtime_state != RuntimeState::NeedsAttention
        {
            anyhow::bail!(
                "{} has no acknowledged drift requiring a baseline update",
                automation.name
            );
        }
        automation.fingerprint = Some(source.fingerprint.clone());
        automation.runtime_state = RuntimeState::Paused;
        self.save_automation(&automation)?;
        self.append_event(&Event {
            run_id: None,
            occurred_at: Utc::now(),
            event_type: "source.drift.acknowledged".into(),
            payload: serde_json::json!({
                "source_id": source.source_id,
                "expected_fingerprint": expected,
                "observed_fingerprint": source.fingerprint,
                "runtime_state": "paused",
            }),
        })?;
        Ok(automation)
    }

    pub fn list_sources(&self) -> Result<Vec<DiscoveredSource>> {
        let mut statement = self.connection.prepare(
            "SELECT id, provider, native_id, path, enabled, kind, fingerprint, command_json, trigger_json, raw FROM sources ORDER BY provider, native_id",
        )?;
        let rows = statement.query_map([], |row| {
            let command_json: Option<String> = row.get(7)?;
            let trigger_json: String = row.get(8)?;
            Ok(DiscoveredSource {
                source_id: row.get(0)?,
                provider: row.get(1)?,
                native_id: row.get(2)?,
                path: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
                enabled: row.get(4)?,
                kind: row.get(5)?,
                fingerprint: row.get(6)?,
                raw: row.get(9)?,
                command: command_json
                    .map(|json| serde_json::from_str(&json))
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            7,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?,
                trigger: serde_json::from_str(&trigger_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        8,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn save_automation(&self, automation: &Automation) -> Result<()> {
        let revision = i64::try_from(automation.revision)
            .context("automation revision exceeds SQLite integer range")?;
        self.connection.execute(
            "INSERT INTO automations (id, name, ownership, runtime_state, definition_json, revision, source_id, fingerprint, next_run_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, ownership=excluded.ownership,
               runtime_state=excluded.runtime_state, definition_json=excluded.definition_json,
               revision=excluded.revision, source_id=excluded.source_id, fingerprint=excluded.fingerprint,
               next_run_at=excluded.next_run_at",
            params![
                automation.id,
                automation.name,
                ownership_db(automation.ownership),
                runtime_state_db(automation.runtime_state),
                canonical_json(automation)?,
                revision,
                automation.source_id,
                automation.fingerprint,
                automation.next_run_at.map(|d| d.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn transition_runtime_state(
        &self,
        id_or_name: &str,
        desired: RuntimeState,
    ) -> Result<Automation> {
        if !matches!(desired, RuntimeState::Enabled | RuntimeState::Paused) {
            anyhow::bail!("runtime transition only supports enabled or paused");
        }
        let mut automation = self
            .get_automation(id_or_name)?
            .with_context(|| format!("automation not found: {id_or_name}"))?;
        if automation.ownership == Ownership::Observed {
            anyhow::bail!(
                "{} is observed-only; native scheduler state must be changed by its provider",
                automation.name
            );
        }
        if automation.runtime_state == RuntimeState::NeedsAttention {
            anyhow::bail!(
                "{} needs attention before its runtime state can change",
                automation.name
            );
        }
        if automation.runtime_state == desired {
            return Ok(automation);
        }
        let previous = automation.runtime_state;
        automation.runtime_state = desired;
        self.save_automation(&automation)?;
        self.append_event(&Event {
            run_id: None,
            occurred_at: Utc::now(),
            event_type: if desired == RuntimeState::Paused {
                "automation.paused"
            } else {
                "automation.resumed"
            }
            .into(),
            payload: serde_json::json!({
                "automation_id": automation.id,
                "from": previous,
                "to": desired,
            }),
        })?;
        Ok(automation)
    }

    pub fn get_automation(&self, id_or_name: &str) -> Result<Option<Automation>> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT definition_json FROM automations WHERE id = ?1 OR name = ?1 LIMIT 1",
                [id_or_name],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn list_automations(&self) -> Result<Vec<Automation>> {
        let mut statement = self
            .connection
            .prepare("SELECT definition_json FROM automations ORDER BY name")?;
        let rows = statement.query_map([], |row| {
            let json: String = row.get(0)?;
            serde_json::from_str::<Automation>(&json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn count_owners_for_fingerprint(&self, fingerprint: &str) -> Result<u32> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM automations WHERE fingerprint = ?1 AND ownership IN ('adopted', 'managed')",
            [fingerprint],
            |row| row.get(0),
        )?)
    }

    pub fn begin_adoption(&self, tx_id: &str, source_id: &str, snapshot: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO adoption_journal (tx_id, source_id, state, snapshot_json, step, updated_at)
             VALUES (?1, ?2, 'preparing', ?3, 'prepare', ?4)",
            params![tx_id, source_id, snapshot, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn update_adoption(
        &self,
        tx_id: &str,
        state: AdoptionState,
        step: &str,
        error: Option<&str>,
    ) -> Result<()> {
        self.connection.execute(
            "UPDATE adoption_journal SET state = ?2, step = ?3, last_error = ?4, updated_at = ?5 WHERE tx_id = ?1",
            params![tx_id, adoption_state_db(state), step, error, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn adoption(&self, tx_id: &str) -> Result<Option<AdoptionRecord>> {
        let Some(record) = self.get_adoption(tx_id)? else {
            return Ok(None);
        };
        Ok(Some((
            record.source_id,
            record.state,
            serde_json::to_string(&record.snapshot)?,
            record.step,
            record.last_error,
        )))
    }

    pub fn get_adoption(&self, tx_id: &str) -> Result<Option<StoredAdoption>> {
        self.connection
            .query_row(
                "SELECT tx_id, source_id, state, snapshot_json, step, last_error, updated_at
                 FROM adoption_journal WHERE tx_id = ?1",
                [tx_id],
                stored_adoption_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_adoptions(&self, limit: usize) -> Result<Vec<StoredAdoption>> {
        let limit = limit.clamp(1, 500) as i64;
        let mut statement = self.connection.prepare(
            "SELECT tx_id, source_id, state, snapshot_json, step, last_error, updated_at
             FROM adoption_journal ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], stored_adoption_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_approval(
        &self,
        id: &str,
        integration: &str,
        action: &str,
        plan_fingerprint: &str,
        plan: &serde_json::Value,
        request: &serde_json::Value,
        risk: &str,
        reason: &str,
        expires_at: &str,
    ) -> Result<StoredApproval> {
        let created_at = Utc::now().to_rfc3339();
        self.connection.execute(
            "INSERT INTO approvals (id, integration, action, plan_fingerprint, plan_json, request_json, risk, status, reason, created_at, expires_at, decided_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9, ?10, NULL)",
            params![id, integration, action, plan_fingerprint, serde_json::to_string(plan)?, serde_json::to_string(request)?, risk, reason, created_at, expires_at],
        )?;
        self.get_approval(id)?
            .context("approval was not readable after creation")
    }

    pub fn get_approval(&self, id: &str) -> Result<Option<StoredApproval>> {
        self.connection
            .query_row(
                "SELECT id, integration, action, plan_fingerprint, plan_json, request_json, risk, status, reason, created_at, expires_at, decided_at FROM approvals WHERE id = ?1",
                [id],
                stored_approval_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_approvals(&self, limit: usize) -> Result<Vec<StoredApproval>> {
        let limit = limit.clamp(1, 500) as i64;
        let mut statement = self.connection.prepare(
            "SELECT id, integration, action, plan_fingerprint, plan_json, request_json, risk, status, reason, created_at, expires_at, decided_at FROM approvals ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], stored_approval_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn decide_approval(&self, id: &str, decision: &str) -> Result<StoredApproval> {
        if !matches!(decision, "approved" | "rejected") {
            anyhow::bail!("approval decision must be approved or rejected");
        }
        let changed = self.connection.execute(
            "UPDATE approvals SET status = ?2, decided_at = ?3 WHERE id = ?1 AND status = 'pending' AND (?2 = 'rejected' OR expires_at > ?3)",
            params![id, decision, Utc::now().to_rfc3339()],
        )?;
        if changed == 0 {
            anyhow::bail!("approval is missing or no longer pending: {id}");
        }
        self.get_approval(id)?
            .context("approval was not readable after decision")
    }

    /// Atomically consume a currently approved, unexpired approval bound to a
    /// plan fingerprint. This prevents replaying the same destructive grant.
    pub fn consume_approval(&self, id: &str, plan_fingerprint: &str) -> Result<StoredApproval> {
        let now = Utc::now().to_rfc3339();
        let changed = self.connection.execute(
            "UPDATE approvals SET status = 'consumed', decided_at = ?3 WHERE id = ?1 AND plan_fingerprint = ?2 AND status = 'approved' AND expires_at > ?3",
            params![id, plan_fingerprint, now],
        )?;
        if changed == 0 {
            anyhow::bail!(
                "approval is invalid, expired, already consumed, or does not match the plan: {id}"
            );
        }
        self.get_approval(id)?
            .context("approval was not readable after consumption")
    }

    pub fn record_run_start(
        &self,
        run_id: &str,
        automation: &Automation,
        scheduled_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let revision = i64::try_from(automation.revision)
            .context("automation revision exceeds SQLite integer range")?;
        self.connection.execute(
            "INSERT INTO runs (id, automation_id, automation_revision, status, scheduled_at, started_at, automation_snapshot_json, stdout, stderr)
             VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, '', '')",
            params![
                run_id,
                automation.id,
                revision,
                scheduled_at.map(|d| d.to_rfc3339()),
                Utc::now().to_rfc3339(),
                redacted_automation_snapshot(automation)?,
            ],
        )?;
        Ok(())
    }

    /// Atomically admit a run under the automation's overlap policy.
    ///
    /// The scheduler performs an inexpensive read before it calls the service,
    /// but every execution entry point must also use this SQLite transaction so
    /// two daemon/CLI processes cannot both pass a check-then-insert race.
    pub fn try_record_run_start(
        &self,
        run_id: &str,
        automation: &Automation,
        scheduled_at: Option<DateTime<Utc>>,
    ) -> Result<bool> {
        let revision = i64::try_from(automation.revision)
            .context("automation revision exceeds SQLite integer range")?;
        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        let result: anyhow::Result<bool> = (|| {
            if automation.concurrency == crate::core::ConcurrencyPolicy::ForbidOverlap {
                let running: u32 = self.connection.query_row(
                    "SELECT COUNT(*) FROM runs WHERE automation_id = ?1 AND status = 'running'",
                    [&automation.id],
                    |row| row.get(0),
                )?;
                if running > 0 {
                    return Ok(false);
                }
            }
            self.connection.execute(
                "INSERT INTO runs (id, automation_id, automation_revision, status, scheduled_at, started_at, automation_snapshot_json, stdout, stderr)
                 VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, '', '')",
                params![
                    run_id,
                    automation.id,
                    revision,
                    scheduled_at.map(|d| d.to_rfc3339()),
                    Utc::now().to_rfc3339(),
                    redacted_automation_snapshot(automation)?,
                ],
            )?;
            Ok(true)
        })();
        match result {
            Ok(admitted) => {
                self.connection.execute_batch("COMMIT")?;
                Ok(admitted)
            }
            Err(error) => {
                let _ = self.connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn list_runs(&self, limit: usize, automation_id: Option<&str>) -> Result<Vec<StoredRun>> {
        let limit = limit.clamp(1, 500) as i64;
        let mut statement = if automation_id.is_some() {
            self.connection.prepare(
                "SELECT id, automation_id, automation_revision, automation_snapshot_json,
                        status, scheduled_at, started_at, ended_at, exit_code
                 FROM runs WHERE automation_id = ?1 ORDER BY started_at DESC LIMIT ?2",
            )?
        } else {
            self.connection.prepare(
                "SELECT id, automation_id, automation_revision, automation_snapshot_json,
                        status, scheduled_at, started_at, ended_at, exit_code
                 FROM runs ORDER BY started_at DESC LIMIT ?1",
            )?
        };
        let rows = if let Some(automation_id) = automation_id {
            statement.query_map(params![automation_id, limit], stored_run_from_row)?
        } else {
            statement.query_map([limit], stored_run_from_row)?
        };
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_run_logs(&self, run_id: &str) -> Result<Option<StoredRunLogs>> {
        self.connection
            .query_row(
                "SELECT id, automation_id, status, stdout, stderr FROM runs WHERE id = ?1",
                [run_id],
                |row| {
                    Ok(StoredRunLogs {
                        run_id: row.get(0)?,
                        automation_id: row.get(1)?,
                        status: row.get(2)?,
                        stdout: row.get(3)?,
                        stderr: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn record_run_end(&self, result: &RunResult) -> Result<()> {
        self.connection.execute(
            "UPDATE runs SET status = ?2, ended_at = ?3, exit_code = ?4, stdout = ?5, stderr = ?6 WHERE id = ?1",
            params![result.run_id, result.status, Utc::now().to_rfc3339(), result.exit_code, result.stdout, result.stderr],
        )?;
        Ok(())
    }

    pub fn count_running_runs(&self, automation_id: &str) -> Result<u32> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM runs WHERE automation_id = ?1 AND status = 'running'",
            [automation_id],
            |row| row.get(0),
        )?)
    }

    pub fn recover_running_runs(&self) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM runs WHERE status = 'running' ORDER BY started_at")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for run_id in &ids {
            let changed = self.connection.execute(
                "UPDATE runs SET status = 'interrupted', ended_at = ?2 WHERE id = ?1 AND status = 'running'",
                params![run_id, Utc::now().to_rfc3339()],
            )?;
            if changed == 1 {
                self.append_event(&Event {
                    run_id: Some(run_id.clone()),
                    occurred_at: Utc::now(),
                    event_type: "run.interrupted".into(),
                    payload: serde_json::json!({"reason": "daemon_restart"}),
                })?;
            }
        }
        Ok(ids)
    }

    pub fn append_event(&self, event: &Event) -> Result<i64> {
        self.connection.execute(
            "INSERT INTO events (run_id, occurred_at, type, payload_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                event.run_id,
                event.occurred_at.to_rfc3339(),
                event.event_type,
                serde_json::to_string(&event.payload)?
            ],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn list_events(&self, limit: usize) -> Result<Vec<StoredEvent>> {
        let limit = limit.clamp(1, 500) as i64;
        let mut statement = self.connection.prepare(
            "SELECT seq, run_id, occurred_at, type, payload_json FROM events ORDER BY seq DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            let payload: String = row.get(4)?;
            Ok(StoredEvent {
                seq: row.get(0)?,
                run_id: row.get(1)?,
                occurred_at: row.get(2)?,
                event_type: row.get(3)?,
                payload: serde_json::from_str(&payload).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn record_github_snapshot(
        &self,
        watch_key: &str,
        repo: &str,
        kind: &str,
        pull_number: Option<u64>,
        fingerprint: &str,
        snapshot: &serde_json::Value,
    ) -> Result<Option<String>> {
        let previous = self
            .connection
            .query_row(
                "SELECT fingerprint FROM github_snapshots WHERE watch_key = ?1",
                [watch_key],
                |row| row.get(0),
            )
            .optional()?;
        self.connection.execute(
            "INSERT INTO github_snapshots (watch_key, repo, kind, pull_number, fingerprint, snapshot_json, observed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(watch_key) DO UPDATE SET repo=excluded.repo, kind=excluded.kind,
               pull_number=excluded.pull_number, fingerprint=excluded.fingerprint,
               snapshot_json=excluded.snapshot_json, observed_at=excluded.observed_at",
            params![
                watch_key,
                repo,
                kind,
                pull_number.map(|value| value as i64),
                fingerprint,
                serde_json::to_string(snapshot)?,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(previous)
    }

    /// Return a bounded, read-only aggregation of existing attention signals.
    pub fn list_inbox(&self, limit: usize) -> Result<Vec<InboxItem>> {
        let limit = limit.clamp(1, 500);
        let mut items = Vec::new();
        for automation in self.list_automations()? {
            if automation.runtime_state == RuntimeState::NeedsAttention {
                items.push(InboxItem {
                    id: format!("automation:{}", automation.id),
                    kind: "automation_attention".into(),
                    severity: "high".into(),
                    status: "needs_attention".into(),
                    title: automation.name.clone(),
                    created_at: None,
                    detail: serde_json::json!({
                        "automation_id": automation.id,
                        "ownership": automation.ownership,
                        "source_id": automation.source_id,
                        "fingerprint": automation.fingerprint,
                    }),
                });
            }
        }
        for adoption in self.list_adoptions(500)?.into_iter() {
            if !adoption.state.is_terminal() {
                items.push(InboxItem {
                    id: format!("adoption:{}", adoption.tx_id),
                    kind: "adoption_recovery".into(),
                    severity: "critical".into(),
                    status: serde_json::to_value(adoption.state)?
                        .as_str()
                        .unwrap_or("unknown")
                        .to_owned(),
                    title: format!("adoption {} needs recovery", adoption.source_id),
                    created_at: Some(adoption.updated_at.clone()),
                    detail: serde_json::json!({
                        "tx_id": adoption.tx_id,
                        "source_id": adoption.source_id,
                        "state": adoption.state,
                        "step": adoption.step,
                        "last_error": adoption.last_error,
                    }),
                });
            }
        }
        let now = Utc::now().to_rfc3339();
        for approval in self.list_approvals(500)?.into_iter() {
            if approval.status == "pending" {
                let expired = approval.expires_at <= now;
                items.push(InboxItem {
                    id: format!("approval:{}", approval.id),
                    kind: "integration_approval".into(),
                    severity: "high".into(),
                    status: if expired { "expired" } else { "pending" }.into(),
                    title: format!("approval · {} · {}", approval.integration, approval.action),
                    created_at: Some(approval.created_at.clone()),
                    detail: serde_json::json!({
                        "approval_id": approval.id,
                        "integration": approval.integration,
                        "action": approval.action,
                        "risk": approval.risk,
                        "plan_fingerprint": approval.plan_fingerprint,
                        "expires_at": approval.expires_at,
                    }),
                });
            }
        }
        for run in self.list_runs(500, None)?.into_iter() {
            if matches!(run.status.as_str(), "failed" | "timed_out" | "interrupted") {
                items.push(InboxItem {
                    id: format!("run:{}", run.id),
                    kind: "run_failure".into(),
                    severity: "high".into(),
                    status: run.status.clone(),
                    title: format!("automation {}", run.automation_id),
                    created_at: Some(run.ended_at.clone().unwrap_or(run.started_at.clone())),
                    detail: serde_json::json!({
                        "run_id": run.id,
                        "automation_id": run.automation_id,
                        "automation_revision": run.automation_revision,
                        "scheduled_at": run.scheduled_at,
                        "exit_code": run.exit_code,
                    }),
                });
            }
        }
        for event in self.list_events(500)?.into_iter() {
            if event.event_type == "integration.attention" {
                let integration = event
                    .payload
                    .get("integration")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("integration");
                let action = event
                    .payload
                    .get("action")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("action");
                items.push(InboxItem {
                    id: format!("integration:{}:{}:{}", integration, action, event.seq),
                    kind: "integration_attention".into(),
                    severity: "high".into(),
                    status: "needs_attention".into(),
                    title: format!("{integration} · {action}"),
                    created_at: Some(event.occurred_at),
                    detail: event.payload,
                });
            }
        }
        items.sort_by(|left, right| {
            inbox_severity_rank(&right.severity)
                .cmp(&inbox_severity_rank(&left.severity))
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        items.truncate(limit);
        Ok(items)
    }

    pub fn record_metric(&self, metric: &Metric) -> Result<()> {
        self.connection.execute(
            "INSERT INTO metrics (id, run_id, key, value, unit, source, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![metric.id, metric.run_id, metric.key, metric.value, metric.unit, metric.source, metric.recorded_at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn list_metrics(&self) -> Result<Vec<Metric>> {
        let mut statement = self.connection.prepare(
            "SELECT id, run_id, key, value, unit, source, recorded_at FROM metrics ORDER BY recorded_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let recorded_at: String = row.get(6)?;
            Ok(Metric {
                id: row.get(0)?,
                run_id: row.get(1)?,
                key: row.get(2)?,
                value: row.get(3)?,
                unit: row.get(4)?,
                source: row.get(5)?,
                recorded_at: DateTime::parse_from_rfc3339(&recorded_at)
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?
                    .with_timezone(&Utc),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn inbox_severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 4,
        "high" => 3,
        "R4_DESTRUCTIVE" => 4,
        "R3_SYSTEM_WRITE" => 3,
        "R2_EXTERNAL_WRITE" => 2,
        "R1_WORKSPACE_WRITE" => 1,
        _ => 0,
    }
}

fn stored_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRun> {
    let snapshot: String = row.get(3)?;
    let revision: i64 = row.get(2)?;
    Ok(StoredRun {
        id: row.get(0)?,
        automation_id: row.get(1)?,
        automation_revision: revision as u64,
        automation_snapshot: serde_json::from_str(&snapshot).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        status: row.get(4)?,
        scheduled_at: row.get(5)?,
        started_at: row.get(6)?,
        ended_at: row.get(7)?,
        exit_code: row.get(8)?,
    })
}

fn redacted_automation_snapshot(automation: &Automation) -> Result<String> {
    let mut snapshot = automation.clone();
    for step in &mut snapshot.steps {
        for value in step.command.env.values_mut() {
            *value = "[REDACTED]".into();
        }
    }
    canonical_json(&snapshot)
}

fn stored_adoption_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAdoption> {
    let state: String = row.get(2)?;
    let snapshot: String = row.get(3)?;
    Ok(StoredAdoption {
        tx_id: row.get(0)?,
        source_id: row.get(1)?,
        state: serde_json::from_value(serde_json::Value::String(state)).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        snapshot: serde_json::from_str(&snapshot).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        step: row.get(4)?,
        last_error: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn stored_approval_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredApproval> {
    let plan: String = row.get(4)?;
    let request: String = row.get(5)?;
    Ok(StoredApproval {
        id: row.get(0)?,
        integration: row.get(1)?,
        action: row.get(2)?,
        plan_fingerprint: row.get(3)?,
        plan: serde_json::from_str(&plan).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        request: serde_json::from_str(&request).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        risk: row.get(6)?,
        status: row.get(7)?,
        reason: row.get(8)?,
        created_at: row.get(9)?,
        expires_at: row.get(10)?,
        decided_at: row.get(11)?,
    })
}

fn ownership_db(value: crate::core::Ownership) -> &'static str {
    match value {
        crate::core::Ownership::Observed => "observed",
        crate::core::Ownership::Adopted => "adopted",
        crate::core::Ownership::Managed => "managed",
    }
}

fn runtime_state_db(value: crate::core::RuntimeState) -> &'static str {
    match value {
        crate::core::RuntimeState::Enabled => "enabled",
        crate::core::RuntimeState::Paused => "paused",
        crate::core::RuntimeState::Running => "running",
        crate::core::RuntimeState::Degraded => "degraded",
        crate::core::RuntimeState::NeedsAttention => "needs_attention",
    }
}

fn adoption_state_db(value: AdoptionState) -> &'static str {
    match value {
        AdoptionState::Preparing => "preparing",
        AdoptionState::NativeDisabled => "native_disabled",
        AdoptionState::InternalEnabled => "internal_enabled",
        AdoptionState::Committed => "committed",
        AdoptionState::RollingBack => "rolling_back",
        AdoptionState::RolledBack => "rolled_back",
        AdoptionState::NeedsAttention => "needs_attention",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        CommandSpec, DiscoveredSource, Metric, Ownership, RuntimeState, StepSpec, Trigger,
    };
    use std::{sync::Arc, sync::Barrier, thread};

    fn automation() -> Automation {
        Automation {
            id: "a1".into(),
            name: "demo".into(),
            ownership: Ownership::Managed,
            steps: vec![StepSpec {
                id: "main".into(),
                command: CommandSpec::argv("echo", ["ok"]),
                responses: None,
            }],
            trigger: Trigger::Manual,
            ..Automation::default()
        }
    }

    #[test]
    fn stores_revision_snapshot_and_run_lifecycle() {
        let registry = Registry::in_memory().unwrap();
        let automation = automation();
        registry.save_automation(&automation).unwrap();
        let loaded = registry.get_automation("demo").unwrap().unwrap();
        assert_eq!(loaded.steps[0].command.args, ["ok"]);
        let mut run_automation = loaded.clone();
        run_automation.steps[0]
            .command
            .env
            .insert("TOKEN".into(), "super-secret".into());
        registry
            .record_run_start("run1", &run_automation, None)
            .unwrap();
        let runs = registry.list_runs(10, Some("a1")).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].automation_revision, loaded.revision);
        assert_eq!(
            runs[0].automation_snapshot["steps"][0]["command"]["args"],
            serde_json::json!(["ok"])
        );
        assert_eq!(
            runs[0].automation_snapshot["steps"][0]["command"]["env"]["TOKEN"],
            "[REDACTED]"
        );
        registry
            .record_run_end(&RunResult {
                run_id: "run1".into(),
                status: "succeeded".into(),
                exit_code: Some(0),
                stdout: "ok\n".into(),
                stderr: String::new(),
                duration_ms: 1,
            })
            .unwrap();
        let logs = registry.get_run_logs("run1").unwrap().unwrap();
        assert_eq!(logs.status, "succeeded");
        assert_eq!(logs.stdout, "ok\n");
        assert!(registry.get_run_logs("missing").unwrap().is_none());
        let seq = registry
            .append_event(&Event {
                run_id: Some("run1".into()),
                occurred_at: Utc::now(),
                event_type: "executor.command.completed".into(),
                payload: serde_json::json!({"exit_code": 0}),
            })
            .unwrap();
        assert!(seq > 0);
    }

    #[test]
    fn rejects_revisions_that_do_not_fit_sqlite_integer() {
        let registry = Registry::in_memory().unwrap();
        let mut automation = automation();
        automation.revision = i64::MAX as u64 + 1;

        assert!(registry.save_automation(&automation).is_err());
        assert!(
            registry
                .record_run_start("run-too-large", &automation, None)
                .is_err()
        );
        assert!(
            registry
                .try_record_run_start("run-too-large-atomic", &automation, None)
                .is_err()
        );
    }

    #[test]
    fn approval_is_persisted_decided_and_consumed_once() {
        let registry = Registry::in_memory().unwrap();
        let approval = registry
            .create_approval(
                "approval_test",
                "mole",
                "clean",
                "sha256:plan",
                &serde_json::json!({"action":"clean"}),
                &serde_json::json!({"dry_run":false}),
                "destructive",
                "operator approval required",
                &(Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
            )
            .unwrap();
        assert_eq!(approval.status, "pending");
        assert_eq!(registry.list_approvals(10).unwrap().len(), 1);
        assert_eq!(
            registry
                .decide_approval("approval_test", "approved")
                .unwrap()
                .status,
            "approved"
        );
        assert_eq!(
            registry
                .consume_approval("approval_test", "sha256:plan")
                .unwrap()
                .status,
            "consumed"
        );
        assert!(
            registry
                .consume_approval("approval_test", "sha256:plan")
                .is_err()
        );
    }

    #[test]
    fn inbox_aggregates_attention_signals_with_a_bound() {
        let registry = Registry::in_memory().unwrap();
        let mut attention = automation();
        attention.id = "attention".into();
        attention.name = "attention".into();
        attention.runtime_state = RuntimeState::NeedsAttention;
        registry.save_automation(&attention).unwrap();
        registry
            .begin_adoption("adopt-inbox", "cron:line-1", "{}")
            .unwrap();
        registry
            .record_run_start("run-inbox", &attention, None)
            .unwrap();
        registry
            .record_run_end(&RunResult {
                run_id: "run-inbox".into(),
                status: "failed".into(),
                exit_code: Some(1),
                stdout: String::new(),
                stderr: "failed".into(),
                duration_ms: 1,
            })
            .unwrap();

        let inbox = registry.list_inbox(10).unwrap();
        assert_eq!(inbox.len(), 3);
        assert!(inbox.iter().any(|item| item.kind == "automation_attention"));
        assert!(inbox.iter().any(|item| item.kind == "adoption_recovery"));
        assert!(inbox.iter().any(|item| item.kind == "run_failure"));
        assert_eq!(registry.list_inbox(2).unwrap().len(), 2);
    }

    #[test]
    fn atomic_run_admission_allows_one_winner_across_registry_connections() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("admission.sqlite3");
        let automation = automation();
        Registry::open(&path)
            .unwrap()
            .save_automation(&automation)
            .unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|index| {
                let path = path.clone();
                let automation = automation.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    let registry = Registry::open(&path).unwrap();
                    barrier.wait();
                    registry
                        .try_record_run_start(&format!("race-{index}"), &automation, None)
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let admitted = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|admitted| *admitted)
            .count();
        assert_eq!(admitted, 1);
        assert_eq!(
            Registry::open(&path)
                .unwrap()
                .count_running_runs(&automation.id)
                .unwrap(),
            1
        );
    }

    #[test]
    fn recovers_stale_running_runs_as_interrupted_with_audit_evidence() {
        let registry = Registry::in_memory().unwrap();
        let automation = automation();
        registry.save_automation(&automation).unwrap();
        registry
            .record_run_start("stale-run", &automation, None)
            .unwrap();
        let recovered = registry.recover_running_runs().unwrap();
        assert_eq!(recovered, vec!["stale-run"]);
        let run = registry.list_runs(1, None).unwrap().pop().unwrap();
        assert_eq!(run.status, "interrupted");
        assert!(
            registry
                .list_events(10)
                .unwrap()
                .iter()
                .any(|event| event.event_type == "run.interrupted")
        );
        assert!(registry.recover_running_runs().unwrap().is_empty());
    }

    #[test]
    fn migrates_existing_run_table_for_revision_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.sqlite3");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE runs (
                    id TEXT PRIMARY KEY,
                    automation_id TEXT NOT NULL,
                    automation_revision INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    scheduled_at TEXT,
                    started_at TEXT NOT NULL,
                    ended_at TEXT,
                    exit_code INTEGER,
                    stdout TEXT NOT NULL,
                    stderr TEXT NOT NULL
                );",
            )
            .unwrap();
        drop(connection);

        let registry = Registry::open(&path).unwrap();
        let automation = automation();
        registry.save_automation(&automation).unwrap();
        registry
            .record_run_start("legacy-run", &automation, None)
            .unwrap();
        let runs = registry.list_runs(1, None).unwrap();
        assert_eq!(runs[0].automation_snapshot["id"], "a1");
    }

    #[test]
    fn transitions_owned_runtime_state_and_records_audit_event() {
        let registry = Registry::in_memory().unwrap();
        let automation = automation();
        registry.save_automation(&automation).unwrap();
        let paused = registry
            .transition_runtime_state("demo", RuntimeState::Paused)
            .unwrap();
        assert_eq!(paused.runtime_state, RuntimeState::Paused);
        let resumed = registry
            .transition_runtime_state("a1", RuntimeState::Enabled)
            .unwrap();
        assert_eq!(resumed.runtime_state, RuntimeState::Enabled);
        let events = registry.list_events(10).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "automation.resumed");
        assert_eq!(events[1].event_type, "automation.paused");
    }

    #[test]
    fn deduplicates_unrunnable_source_events_after_first_attention_mark() {
        let registry = Registry::in_memory().unwrap();
        let direct = DiscoveredSource {
            source_id: "launchd:unrunnable".into(),
            provider: "launchd".into(),
            native_id: "unrunnable".into(),
            path: None,
            enabled: true,
            kind: "task".into(),
            fingerprint: "sha256:direct".into(),
            command: Some(CommandSpec::argv("/bin/echo", ["ok"])),
            trigger: Trigger::Manual,
            raw: "direct".into(),
        };
        registry.reconcile_discovered_source(&direct).unwrap();
        let shell = DiscoveredSource {
            command: Some(CommandSpec::argv("/bin/sh", ["-c", "echo unsafe"])),
            fingerprint: "sha256:shell".into(),
            raw: "shell".into(),
            ..direct
        };
        registry.reconcile_discovered_source(&shell).unwrap();
        registry.reconcile_discovered_source(&shell).unwrap();
        let events = registry
            .list_events(20)
            .unwrap()
            .into_iter()
            .filter(|event| event.event_type == "source.unrunnable")
            .count();
        assert_eq!(events, 1);
    }

    #[test]
    fn observed_runtime_state_cannot_be_changed_by_control_plane() {
        let registry = Registry::in_memory().unwrap();
        let mut automation = automation();
        automation.ownership = Ownership::Observed;
        registry.save_automation(&automation).unwrap();
        assert!(
            registry
                .transition_runtime_state("a1", RuntimeState::Paused)
                .is_err()
        );
    }

    #[test]
    fn needs_attention_state_cannot_be_cleared_by_pause_or_resume() {
        let registry = Registry::in_memory().unwrap();
        let mut automation = automation();
        automation.runtime_state = RuntimeState::NeedsAttention;
        registry.save_automation(&automation).unwrap();
        assert!(
            registry
                .transition_runtime_state("a1", RuntimeState::Paused)
                .is_err()
        );
        assert!(
            registry
                .transition_runtime_state("a1", RuntimeState::Enabled)
                .is_err()
        );
    }

    #[test]
    fn lists_and_inspects_adoption_journal_without_mutating_native_state() {
        let registry = Registry::in_memory().unwrap();
        registry
            .begin_adoption("tx-1", "launchd:test", r#"{"loaded":true}"#)
            .unwrap();
        registry
            .update_adoption(
                "tx-1",
                AdoptionState::NativeDisabled,
                "verify_disabled",
                None,
            )
            .unwrap();
        let inspected = registry.get_adoption("tx-1").unwrap().unwrap();
        assert_eq!(inspected.state, AdoptionState::NativeDisabled);
        assert_eq!(inspected.snapshot["loaded"], true);
        assert_eq!(inspected.step, "verify_disabled");
        let listed = registry.list_adoptions(10).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].tx_id, "tx-1");
        assert!(registry.get_adoption("missing").unwrap().is_none());
    }

    #[test]
    fn persists_usage_metrics_without_inventing_cost() {
        let registry = Registry::in_memory().unwrap();
        registry
            .record_metric(&Metric {
                id: "metric_1".into(),
                run_id: None,
                key: "input_tokens".into(),
                value: 123.0,
                unit: "tokens".into(),
                source: "codex.exec".into(),
                recorded_at: Utc::now(),
            })
            .unwrap();
        let metrics = registry.list_metrics().unwrap();
        assert_eq!(metrics[0].value, 123.0);
        assert_eq!(metrics[0].unit, "tokens");
    }

    #[test]
    fn deduplicates_github_snapshots_by_watch_key_and_fingerprint() {
        let registry = Registry::in_memory().unwrap();
        let snapshot = serde_json::json!({"items": [{"number": 1}]});
        assert_eq!(
            registry
                .record_github_snapshot(
                    "owner/repo:pulls:-",
                    "owner/repo",
                    "pulls",
                    None,
                    "sha256:first",
                    &snapshot,
                )
                .unwrap(),
            None
        );
        assert_eq!(
            registry
                .record_github_snapshot(
                    "owner/repo:pulls:-",
                    "owner/repo",
                    "pulls",
                    None,
                    "sha256:first",
                    &snapshot,
                )
                .unwrap(),
            Some("sha256:first".into())
        );
        assert_eq!(
            registry
                .record_github_snapshot(
                    "owner/repo:pulls:-",
                    "owner/repo",
                    "pulls",
                    None,
                    "sha256:second",
                    &serde_json::json!({"items": [{"number": 2}]}),
                )
                .unwrap(),
            Some("sha256:first".into())
        );
    }

    #[test]
    fn lists_newest_events_with_a_bounded_limit() {
        let registry = Registry::in_memory().unwrap();
        for index in 0..3 {
            registry
                .append_event(&Event {
                    run_id: None,
                    occurred_at: Utc::now(),
                    event_type: format!("test.{index}"),
                    payload: serde_json::json!({"index": index}),
                })
                .unwrap();
        }
        let events = registry.list_events(2).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "test.2");
        assert_eq!(events[1].event_type, "test.1");
        assert_eq!(registry.list_events(0).unwrap().len(), 1);
    }

    #[test]
    fn reconciliation_preserves_owned_state_and_deduplicates_drift_events() {
        let registry = Registry::in_memory().unwrap();
        let source = DiscoveredSource {
            source_id: "launchd:owned".into(),
            provider: "launchd".into(),
            native_id: "owned".into(),
            path: None,
            enabled: true,
            kind: "service".into(),
            fingerprint: "sha256:first".into(),
            command: Some(CommandSpec::argv("/bin/echo", ["ok"])),
            trigger: Trigger::Manual,
            raw: "first".into(),
        };
        assert_eq!(
            registry.reconcile_discovered_source(&source).unwrap(),
            SourceReconciliation::CreatedObserved
        );
        let mut adopted = source.as_observed_automation().unwrap();
        adopted.ownership = Ownership::Adopted;
        adopted.runtime_state = RuntimeState::Enabled;
        registry.save_automation(&adopted).unwrap();
        let changed = DiscoveredSource {
            fingerprint: "sha256:second".into(),
            raw: "second".into(),
            ..source
        };
        assert_eq!(
            registry.reconcile_discovered_source(&changed).unwrap(),
            SourceReconciliation::Drifted
        );
        assert_eq!(
            registry
                .get_automation("launchd:owned")
                .unwrap()
                .unwrap()
                .ownership,
            Ownership::Adopted
        );
        assert_eq!(
            registry
                .get_automation("launchd:owned")
                .unwrap()
                .unwrap()
                .runtime_state,
            RuntimeState::NeedsAttention
        );
        registry.reconcile_discovered_source(&changed).unwrap();
        let drift_events = registry
            .list_events(20)
            .unwrap()
            .into_iter()
            .filter(|event| event.event_type == "source.drifted")
            .count();
        assert_eq!(drift_events, 1);
        let acknowledged = registry.acknowledge_source_drift("owned").unwrap();
        assert_eq!(acknowledged.runtime_state, RuntimeState::Paused);
        assert_eq!(acknowledged.fingerprint.as_deref(), Some("sha256:second"));
        assert!(registry.acknowledge_source_drift("owned").is_err());
    }
}
