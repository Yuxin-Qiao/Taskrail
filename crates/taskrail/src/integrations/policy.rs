use super::model::{ExecutionPlan, RiskClass};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allowed,
    RequiresApproval { reason: String },
    Denied { reason: String },
}

impl PolicyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Policy boundary used by integrations before a plan reaches the executor.
pub trait PolicyEvaluator: Send + Sync {
    fn evaluate(&self, plan: &ExecutionPlan) -> PolicyDecision;
}

/// Conservative default until a durable approval store is connected.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicy;

impl PolicyEvaluator for DefaultPolicy {
    fn evaluate(&self, plan: &ExecutionPlan) -> PolicyDecision {
        if let Err(error) = plan.validate() {
            return PolicyDecision::Denied {
                reason: error.to_string(),
            };
        }
        if matches!(plan.risk, RiskClass::Read) {
            PolicyDecision::Allowed
        } else {
            PolicyDecision::RequiresApproval {
                reason: format!(
                    "{} action {} requires durable operator approval",
                    plan.risk.risk_name(),
                    plan.action
                ),
            }
        }
    }
}

trait RiskName {
    fn risk_name(self) -> &'static str;
}

impl RiskName for RiskClass {
    fn risk_name(self) -> &'static str {
        match self {
            RiskClass::Read => "read-only",
            RiskClass::FilesystemWrite => "filesystem-write",
            RiskClass::NetworkWrite => "network-write",
            RiskClass::SystemWrite => "system-write",
            RiskClass::Destructive => "destructive",
        }
    }
}

pub fn evaluate_plan(plan: &ExecutionPlan) -> anyhow::Result<PolicyDecision> {
    let decision = DefaultPolicy.evaluate(plan);
    if matches!(decision, PolicyDecision::Denied { .. }) {
        anyhow::bail!("integration plan denied: {decision:?}");
    }
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core::CommandSpec, integrations::IntegrationId};

    fn plan(risk: RiskClass) -> ExecutionPlan {
        ExecutionPlan {
            integration: IntegrationId::new("fixture").unwrap(),
            action: "run".into(),
            command: CommandSpec::argv("fixture-tool", ["--json"]),
            environment_refs: Vec::new(),
            risk,
            requires_approval: risk.requires_approval(),
            supports_dry_run: false,
            dry_run: false,
            plan_only: false,
            timeout_seconds: 30,
            verification: None,
        }
    }

    #[test]
    fn default_policy_allows_reads_and_holds_writes() {
        assert_eq!(
            DefaultPolicy.evaluate(&plan(RiskClass::Read)),
            PolicyDecision::Allowed
        );
        assert!(matches!(
            DefaultPolicy.evaluate(&plan(RiskClass::Destructive)),
            PolicyDecision::RequiresApproval { .. }
        ));
    }
}
