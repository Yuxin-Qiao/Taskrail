use super::model::{
    DetectionResult, DoctorResult, ExecutionPlan, IntegrationAction, IntegrationDescriptor,
    IntegrationId, IntegrationResult, ProcessOutput, VerificationResult,
};
use anyhow::Result;
use std::{collections::BTreeMap, sync::Arc};

/// Semantic adapter contract. Implementations describe and parse; they do not
/// execute the action or persist state directly. An optional preflight may
/// perform a bounded, read-only freshness check before policy or execution.
pub trait Integration: Send + Sync {
    fn descriptor(&self) -> &IntegrationDescriptor;
    fn detect(&self) -> DetectionResult;
    fn doctor(&self) -> DoctorResult;
    fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan>;
    fn preflight(&self, _action: &IntegrationAction) -> Result<()> {
        Ok(())
    }
    fn parse(&self, action: &IntegrationAction, output: ProcessOutput)
    -> Result<IntegrationResult>;
    fn verify(
        &self,
        action: &IntegrationAction,
        result: &IntegrationResult,
    ) -> Result<VerificationResult>;
}

#[derive(Default, Clone)]
pub struct IntegrationRegistry {
    integrations: BTreeMap<IntegrationId, Arc<dyn Integration>>,
}

impl IntegrationRegistry {
    pub fn register<I>(&mut self, integration: I) -> Result<()>
    where
        I: Integration + 'static,
    {
        let id = integration.descriptor().id.clone();
        if self.integrations.contains_key(&id) {
            anyhow::bail!("integration already registered: {id}");
        }
        self.integrations.insert(id, Arc::new(integration));
        Ok(())
    }

    pub fn get(&self, id: &IntegrationId) -> Option<Arc<dyn Integration>> {
        self.integrations.get(id).cloned()
    }

    pub fn descriptors(&self) -> Vec<&IntegrationDescriptor> {
        self.integrations
            .values()
            .map(|integration| integration.descriptor())
            .collect()
    }

    pub fn detect(&self) -> Vec<DetectionResult> {
        self.integrations
            .values()
            .map(|integration| integration.detect())
            .collect()
    }

    pub fn doctor(&self) -> Vec<DoctorResult> {
        self.integrations
            .values()
            .map(|integration| integration.doctor())
            .collect()
    }

    pub fn plan(&self, id: &IntegrationId, action: &IntegrationAction) -> Result<ExecutionPlan> {
        let integration = self
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("integration not registered: {id}"))?;
        let plan = integration.plan(action)?;
        if &plan.integration != id || plan.action != action.action {
            anyhow::bail!("integration returned a plan for a different action");
        }
        plan.validate()?;
        Ok(plan)
    }

    pub fn parse(
        &self,
        id: &IntegrationId,
        action: &IntegrationAction,
        output: ProcessOutput,
    ) -> Result<IntegrationResult> {
        let integration = self
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("integration not registered: {id}"))?;
        let result = integration.parse(action, output.bounded(1024 * 1024))?;
        result.ensure_matches(id, action)?;
        Ok(result)
    }

    pub fn verify(
        &self,
        id: &IntegrationId,
        action: &IntegrationAction,
        result: &IntegrationResult,
    ) -> Result<VerificationResult> {
        result.ensure_matches(id, action)?;
        let integration = self
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("integration not registered: {id}"))?;
        integration.verify(action, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CommandSpec;
    use crate::integrations::model::{
        Capability, DetectionStatus, DoctorCheck, DoctorStatus, IntegrationLevel,
        IntegrationStatus, MetricValue, ProcessStatus, RiskClass, VerificationStatus,
    };
    use std::collections::BTreeMap;

    struct FixtureIntegration {
        descriptor: IntegrationDescriptor,
    }

    impl FixtureIntegration {
        fn new() -> Self {
            Self {
                descriptor: IntegrationDescriptor {
                    id: IntegrationId::new("fixture").unwrap(),
                    display_name: "Fixture tool".into(),
                    level: IntegrationLevel::Semantic,
                    capabilities: vec![Capability::new("scan", RiskClass::Read, false)],
                },
            }
        }
    }

    impl Integration for FixtureIntegration {
        fn descriptor(&self) -> &IntegrationDescriptor {
            &self.descriptor
        }

        fn detect(&self) -> DetectionResult {
            DetectionResult {
                integration: self.descriptor.id.clone(),
                status: DetectionStatus::Available,
                executable: Some("fixture-tool".into()),
                version: Some("1.0".into()),
                detail: None,
            }
        }

        fn doctor(&self) -> DoctorResult {
            DoctorResult {
                integration: self.descriptor.id.clone(),
                status: DoctorStatus::Ready,
                checks: vec![DoctorCheck {
                    name: "fixture".into(),
                    ok: true,
                    detail: "fixture is ready".into(),
                }],
            }
        }

        fn plan(&self, action: &IntegrationAction) -> Result<ExecutionPlan> {
            Ok(ExecutionPlan {
                integration: self.descriptor.id.clone(),
                action: action.action.clone(),
                command: CommandSpec::argv("fixture-tool", ["scan", "--json"]),
                environment_refs: Vec::new(),
                risk: RiskClass::Read,
                requires_approval: false,
                supports_dry_run: false,
                dry_run: false,
                plan_only: false,
                timeout_seconds: 30,
                verification: None,
            })
        }

        fn parse(
            &self,
            action: &IntegrationAction,
            output: ProcessOutput,
        ) -> Result<IntegrationResult> {
            let status = if matches!(output.status, ProcessStatus::Succeeded) {
                IntegrationStatus::Succeeded
            } else {
                IntegrationStatus::Failed
            };
            Ok(IntegrationResult {
                integration: self.descriptor.id.clone(),
                action: action.action.clone(),
                status,
                summary: output.stdout.trim().to_owned(),
                metrics: BTreeMap::from([(
                    "duration".into(),
                    MetricValue {
                        value: output.duration_ms as f64,
                        unit: "ms".into(),
                    },
                )]),
                findings: Vec::new(),
                changes: Vec::new(),
                artifacts: Vec::new(),
                raw_output_ref: None,
            })
        }

        fn verify(
            &self,
            _action: &IntegrationAction,
            result: &IntegrationResult,
        ) -> Result<VerificationResult> {
            Ok(VerificationResult {
                status: if result.status == IntegrationStatus::Succeeded {
                    VerificationStatus::Passed
                } else {
                    VerificationStatus::Failed
                },
                checks: Vec::new(),
            })
        }
    }

    #[test]
    fn registry_dispatches_detect_plan_parse_and_verify() {
        let mut registry = IntegrationRegistry::default();
        registry.register(FixtureIntegration::new()).unwrap();
        let id = IntegrationId::new("fixture").unwrap();
        assert_eq!(registry.descriptors().len(), 1);
        assert_eq!(registry.detect()[0].status, DetectionStatus::Available);
        assert_eq!(registry.doctor()[0].status, DoctorStatus::Ready);
        let action = IntegrationAction::new("scan").unwrap();
        let plan = registry.plan(&id, &action).unwrap();
        assert_eq!(plan.command.args, ["scan", "--json"]);
        let result = registry
            .parse(
                &id,
                &action,
                ProcessOutput {
                    status: ProcessStatus::Succeeded,
                    exit_code: Some(0),
                    stdout: "2 findings".into(),
                    stderr: String::new(),
                    duration_ms: 4,
                },
            )
            .unwrap();
        assert_eq!(result.summary, "2 findings");
        assert_eq!(
            registry.verify(&id, &action, &result).unwrap().status,
            VerificationStatus::Passed
        );
    }

    #[test]
    fn registry_rejects_duplicates_and_unknown_integrations() {
        let mut registry = IntegrationRegistry::default();
        registry.register(FixtureIntegration::new()).unwrap();
        assert!(registry.register(FixtureIntegration::new()).is_err());
        let unknown = IntegrationId::new("unknown").unwrap();
        let action = IntegrationAction::new("scan").unwrap();
        assert!(registry.plan(&unknown, &action).is_err());
    }
}
