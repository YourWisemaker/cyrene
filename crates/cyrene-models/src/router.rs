//! Model Router: selects providers, escalates on failure, de-escalates on success.

use cyrene_core::{Model, Tier};
use std::sync::Arc;

/// Why an escalation or de-escalation occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationEvent {
    pub from_tier: Tier,
    pub to_tier: Tier,
    pub reason: String,
}

/// Errors from the router.
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("all configured providers failed")]
    AllProvidersFailed,
    #[error("no providers registered for tier {0:?}")]
    NoProviderForTier(Tier),
}

/// The Model Router: defaults to local, escalates after 2 consecutive failures.
pub struct ModelRouter {
    local: Vec<Arc<dyn Model>>,
    premium: Vec<Arc<dyn Model>>,
    consecutive_failures: u32,
    escalated: bool,
    events: Vec<EscalationEvent>,
}

impl ModelRouter {
    /// Create a router from a list of models, partitioned by tier.
    pub fn new(models: Vec<Arc<dyn Model>>) -> Self {
        let mut local = Vec::new();
        let mut premium = Vec::new();
        for m in models {
            match m.descriptor().tier {
                Tier::Local => local.push(m),
                Tier::Premium => premium.push(m),
            }
        }
        Self {
            local,
            premium,
            consecutive_failures: 0,
            escalated: false,
            events: Vec::new(),
        }
    }

    /// Select the current model based on escalation state.
    pub fn select(&self) -> Result<Arc<dyn Model>, RouterError> {
        if self.escalated {
            self.premium
                .first()
                .cloned()
                .ok_or(RouterError::NoProviderForTier(Tier::Premium))
        } else {
            self.local
                .first()
                .cloned()
                .ok_or(RouterError::NoProviderForTier(Tier::Local))
        }
    }

    /// Report a failure. After 2 consecutive failures, escalate to premium.
    pub fn report_failure(&mut self) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= 2 && !self.escalated && !self.premium.is_empty() {
            self.escalated = true;
            self.events.push(EscalationEvent {
                from_tier: Tier::Local,
                to_tier: Tier::Premium,
                reason: format!(
                    "{} consecutive failures on local provider",
                    self.consecutive_failures
                ),
            });
            self.consecutive_failures = 0;
        }
    }

    /// Report a success. Resets failure count and de-escalates if on premium.
    pub fn report_success(&mut self) {
        self.consecutive_failures = 0;
        if self.escalated {
            self.escalated = false;
            self.events.push(EscalationEvent {
                from_tier: Tier::Premium,
                to_tier: Tier::Local,
                reason: "success on premium; returning to local".to_owned(),
            });
        }
    }

    /// Whether the router is currently escalated to premium.
    #[must_use]
    pub fn is_escalated(&self) -> bool {
        self.escalated
    }

    /// Returns logged escalation/de-escalation events.
    #[must_use]
    pub fn events(&self) -> &[EscalationEvent] {
        &self.events
    }

    /// Check if all providers have been exhausted (both tiers failed).
    pub fn all_failed(&self) -> Result<(), RouterError> {
        if self.escalated && self.consecutive_failures >= 2 {
            Err(RouterError::AllProvidersFailed)
        } else {
            Ok(())
        }
    }

    /// Returns the consecutive failure count.
    #[must_use]
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use cyrene_core::{
        FinishReason, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Money, TokenUsage,
    };

    struct FakeModel {
        tier: Tier,
        alias: String,
    }

    #[async_trait]
    impl Model for FakeModel {
        fn descriptor(&self) -> ModelDescriptor {
            ModelDescriptor {
                alias: self.alias.clone(),
                tier: self.tier,
                input_price: Money::zero("USD"),
                output_price: Money::zero("USD"),
            }
        }
        async fn complete(&self, _req: ModelRequest) -> Result<ModelResponse, ModelError> {
            Ok(ModelResponse::new(
                "ok",
                TokenUsage::new(10, 10),
                FinishReason::Stop,
            ))
        }
        async fn embed(&self, _text: &str) -> Result<Vec<f32>, ModelError> {
            Ok(vec![])
        }
    }

    fn make_router() -> ModelRouter {
        let local: Arc<dyn Model> = Arc::new(FakeModel {
            tier: Tier::Local,
            alias: "local".to_owned(),
        });
        let premium: Arc<dyn Model> = Arc::new(FakeModel {
            tier: Tier::Premium,
            alias: "premium".to_owned(),
        });
        ModelRouter::new(vec![local, premium])
    }

    #[test]
    fn default_selects_local() {
        let router = make_router();
        let m = router.select().unwrap();
        assert_eq!(m.descriptor().tier, Tier::Local);
        assert!(!router.is_escalated());
    }

    #[test]
    fn one_failure_stays_local() {
        let mut router = make_router();
        router.report_failure();
        assert!(!router.is_escalated());
        assert_eq!(router.select().unwrap().descriptor().tier, Tier::Local);
    }

    #[test]
    fn two_failures_escalate_to_premium() {
        let mut router = make_router();
        router.report_failure();
        router.report_failure();
        assert!(router.is_escalated());
        assert_eq!(router.select().unwrap().descriptor().tier, Tier::Premium);
    }

    #[test]
    fn success_after_escalation_de_escalates() {
        let mut router = make_router();
        router.report_failure();
        router.report_failure();
        assert!(router.is_escalated());
        router.report_success();
        assert!(!router.is_escalated());
        assert_eq!(router.select().unwrap().descriptor().tier, Tier::Local);
    }

    #[test]
    fn events_are_logged() {
        let mut router = make_router();
        router.report_failure();
        router.report_failure(); // escalate
        router.report_success(); // de-escalate
        assert_eq!(router.events().len(), 2);
        assert_eq!(router.events()[0].to_tier, Tier::Premium);
        assert_eq!(router.events()[1].to_tier, Tier::Local);
    }

    #[test]
    fn all_failed_after_premium_exhausted() {
        let mut router = make_router();
        router.report_failure();
        router.report_failure(); // escalate to premium
        router.report_failure();
        router.report_failure(); // 2 failures on premium
        assert!(router.all_failed().is_err());
    }

    #[test]
    fn reset_after_de_escalation() {
        let mut router = make_router();
        router.report_failure();
        router.report_failure(); // escalate
        router.report_success(); // de-escalate
        assert_eq!(router.consecutive_failures(), 0);
        // One more failure should not escalate again immediately
        router.report_failure();
        assert!(!router.is_escalated());
    }

    #[test]
    fn no_premium_providers_stays_local_on_failure() {
        let local: Arc<dyn Model> = Arc::new(FakeModel {
            tier: Tier::Local,
            alias: "local".to_owned(),
        });
        let mut router = ModelRouter::new(vec![local]);
        router.report_failure();
        router.report_failure();
        // Can't escalate — no premium providers
        assert!(!router.is_escalated());
    }
}
