use crate::core::{ApprovalState, Risk};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    NeedsApproval,
    Deny,
}

pub fn decide(
    risk: Risk,
    max_risk: Risk,
    approval_required: bool,
    state: Option<ApprovalState>,
) -> GateDecision {
    if risk > max_risk {
        return GateDecision::Deny;
    }
    if !risk.requires_approval() && !approval_required {
        return GateDecision::Allow;
    }
    match state {
        Some(ApprovalState::Approved) => GateDecision::Allow,
        Some(ApprovalState::Rejected | ApprovalState::Expired) => GateDecision::Deny,
        Some(ApprovalState::Pending) | None => GateDecision::NeedsApproval,
    }
}

pub fn require_approved(
    risk: Risk,
    max_risk: Risk,
    approval_required: bool,
    state: Option<ApprovalState>,
) -> Result<()> {
    match decide(risk, max_risk, approval_required, state) {
        GateDecision::Allow => Ok(()),
        GateDecision::NeedsApproval => anyhow::bail!("operation requires a human approval"),
        GateDecision::Deny => anyhow::bail!("operation denied by policy"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_is_automatic() {
        assert_eq!(
            decide(Risk::R0Read, Risk::R0Read, false, None),
            GateDecision::Allow
        );
    }

    #[test]
    fn external_write_requires_approval() {
        assert_eq!(
            decide(Risk::R2ExternalWrite, Risk::R2ExternalWrite, false, None),
            GateDecision::NeedsApproval
        );
        assert_eq!(
            decide(
                Risk::R2ExternalWrite,
                Risk::R2ExternalWrite,
                false,
                Some(ApprovalState::Approved)
            ),
            GateDecision::Allow
        );
        assert_eq!(
            decide(
                Risk::R2ExternalWrite,
                Risk::R1WorkspaceWrite,
                false,
                Some(ApprovalState::Approved)
            ),
            GateDecision::Deny
        );
    }
}
