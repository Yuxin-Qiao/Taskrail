use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A system job identity accepted by a future privileged helper.
///
/// This is intentionally not a command line or an arbitrary filesystem path.
/// The eventual helper can map these identities to a fixed allowlist of
/// Service Management operations without exposing a generic root executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnownSystemJob {
    LaunchAgent { label: String },
    LaunchDaemon { label: String },
}

impl KnownSystemJob {
    pub fn launch_agent(label: impl Into<String>) -> Result<Self> {
        Self::validated(Self::LaunchAgent {
            label: label.into(),
        })
    }

    pub fn launch_daemon(label: impl Into<String>) -> Result<Self> {
        Self::validated(Self::LaunchDaemon {
            label: label.into(),
        })
    }

    fn validated(job: Self) -> Result<Self> {
        let label = match &job {
            Self::LaunchAgent { label } | Self::LaunchDaemon { label } => label,
        };
        if label.is_empty()
            || label.len() > 255
            || label.contains('/')
            || label.contains('\\')
            || label.chars().any(char::is_whitespace)
            || label.contains('\0')
        {
            anyhow::bail!("invalid system job label")
        }
        Ok(job)
    }

    pub fn label(&self) -> &str {
        match self {
            Self::LaunchAgent { label } | Self::LaunchDaemon { label } => label,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemJobSnapshot {
    pub job: KnownSystemJob,
    pub loaded: bool,
    pub enabled: bool,
    pub path: Option<PathBuf>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivilegedMutationReceipt {
    pub job: KnownSystemJob,
    pub operation: String,
    pub changed: bool,
    pub detail: String,
}

/// Narrow boundary for operations that may eventually require elevated
/// privileges. Implementations must keep the allowlist in code; there is no
/// `exec(command)` escape hatch by design.
pub trait PrivilegedHelper: Send + Sync {
    fn query_system_job(&self, job: &KnownSystemJob) -> Result<SystemJobSnapshot>;
    fn enable_system_job(&self, job: &KnownSystemJob) -> Result<PrivilegedMutationReceipt>;
    fn disable_system_job(&self, job: &KnownSystemJob) -> Result<PrivilegedMutationReceipt>;
    fn read_known_plist(&self, job: &KnownSystemJob) -> Result<Vec<u8>>;
}

/// Safe default until a separately installed, reviewed helper is available.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPrivilegedHelper;

impl NoPrivilegedHelper {
    fn unavailable(operation: &str) -> anyhow::Error {
        anyhow::anyhow!("privileged helper is not installed; refusing {operation}")
    }
}

impl PrivilegedHelper for NoPrivilegedHelper {
    fn query_system_job(&self, _job: &KnownSystemJob) -> Result<SystemJobSnapshot> {
        Err(Self::unavailable("query_system_job"))
    }

    fn enable_system_job(&self, _job: &KnownSystemJob) -> Result<PrivilegedMutationReceipt> {
        Err(Self::unavailable("enable_system_job"))
    }

    fn disable_system_job(&self, _job: &KnownSystemJob) -> Result<PrivilegedMutationReceipt> {
        Err(Self::unavailable("disable_system_job"))
    }

    fn read_known_plist(&self, _job: &KnownSystemJob) -> Result<Vec<u8>> {
        Err(Self::unavailable("read_known_plist"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_identity_rejects_path_and_whitespace_injection() {
        assert!(KnownSystemJob::launch_daemon("com.example.auto").is_ok());
        assert!(KnownSystemJob::launch_daemon("../com.example.auto").is_err());
        assert!(KnownSystemJob::launch_daemon("com.example auto").is_err());
        assert!(KnownSystemJob::launch_daemon("").is_err());
    }

    #[test]
    fn default_helper_fails_closed_for_every_typed_operation() {
        let job = KnownSystemJob::launch_agent("com.example.auto").unwrap();
        let helper = NoPrivilegedHelper;
        assert!(helper.query_system_job(&job).is_err());
        assert!(helper.enable_system_job(&job).is_err());
        assert!(helper.disable_system_job(&job).is_err());
        assert!(helper.read_known_plist(&job).is_err());
    }
}
