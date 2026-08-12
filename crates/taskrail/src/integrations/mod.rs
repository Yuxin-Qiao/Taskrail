//! Semantic adapters for external automation-friendly CLI tools.
//!
//! Integrations describe what a tool means and how risky an action is. They do
//! not spawn processes themselves. An integration produces an [`ExecutionPlan`]
//! and later parses the bounded output of the existing Taskrail executor.

mod model;
mod mole;
mod policy;
mod registry;

pub use model::{
    ArtifactRef, Capability, Change, DetectionResult, DetectionStatus, DoctorCheck, DoctorResult,
    DoctorStatus, EnvironmentRef, ExecutionPlan, Finding, IntegrationAction, IntegrationDescriptor,
    IntegrationId, IntegrationLevel, IntegrationResult, IntegrationStatus, MetricValue,
    ProcessOutput, ProcessStatus, RiskClass, VerificationCheck, VerificationPlan,
    VerificationResult, VerificationStatus,
};
pub use mole::MoleIntegration;
pub use policy::{DefaultPolicy, PolicyDecision, PolicyEvaluator, evaluate_plan};
pub use registry::{Integration, IntegrationRegistry};
