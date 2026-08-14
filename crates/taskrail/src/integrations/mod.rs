//! Semantic adapters for external automation-friendly CLI tools.
//!
//! Integrations describe what a tool means and how risky an action is. They do
//! not spawn processes themselves. An integration produces an [`ExecutionPlan`]
//! and later parses the bounded output of the existing Taskrail executor.

mod github;
mod helpers;
mod homebrew;
mod mas;
mod model;
mod mole;
mod policy;
mod rclone;
mod registry;
mod restic;
mod security;
mod shortcuts;
mod topgrade;
mod vibecleaner;

pub use github::GithubIntegration;
pub use homebrew::HomebrewIntegration;
pub use mas::MasIntegration;
pub use model::{
    ArtifactRef, Capability, Change, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult,
    DoctorStatus, EnvironmentRef, ExecutionPlan, Finding, IntegrationAction, IntegrationDescriptor,
    IntegrationId, IntegrationLevel, IntegrationResult, IntegrationStatus, MetricValue,
    ProcessOutput, ProcessStatus, RiskClass, VerificationCheck, VerificationPlan,
    VerificationResult, VerificationStatus,
};
pub use mole::MoleIntegration;
pub use policy::{DefaultPolicy, PolicyDecision, PolicyEvaluator, evaluate_plan};
pub use rclone::RcloneIntegration;
pub use registry::{Integration, IntegrationRegistry};
pub use restic::ResticIntegration;
pub use security::{SecurityIntegration, SecurityTool};
pub use shortcuts::ShortcutsIntegration;
pub use topgrade::TopgradeIntegration;
pub use vibecleaner::VibeCleanerIntegration;

/// Construct the built-in adapter set used by the daemon, CLI, and MCP
/// boundary. Keeping this in one place prevents a surface from silently
/// exposing a different integration catalog from another surface.
pub fn built_in_registry() -> anyhow::Result<IntegrationRegistry> {
    let mut registry = IntegrationRegistry::default();
    registry.register(MoleIntegration::default())?;
    registry.register(ResticIntegration::default())?;
    registry.register(RcloneIntegration::default())?;
    registry.register(GithubIntegration::default())?;
    registry.register(HomebrewIntegration::default())?;
    registry.register(MasIntegration::default())?;
    registry.register(SecurityIntegration::osv())?;
    registry.register(SecurityIntegration::gitleaks())?;
    registry.register(SecurityIntegration::trivy())?;
    registry.register(ShortcutsIntegration::default())?;
    registry.register(TopgradeIntegration::default())?;
    registry.register(VibeCleanerIntegration::default())?;
    Ok(registry)
}
