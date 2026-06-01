//! Property tests for Budget Guard (Property 5) and Model Router (Property 10).

use cyrene_core::{Budget, Money, Tier};
use cyrene_models::{BudgetAction, BudgetGuard, ModelRouter};
use proptest::prelude::*;
use std::sync::Arc;
use std::time::Duration;

// ─── Fake Model for Router tests ─────────────────────────────────────────────

use async_trait::async_trait;
use cyrene_core::{
    FinishReason, Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, TokenUsage,
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

// ─── Property 5: Budget never exceeded ───────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Property 5: For any sequence of step costs, cumulative usage never
    /// exceeds any configured cap — the guard validates projected post-step
    /// usage before authorizing each step.
    ///
    /// **Validates: Requirements 13.6**
    #[test]
    fn prop5_budget_never_exceeded(
        cap_minor in 100i64..10000,
        token_cap in 100u64..100000,
        costs in prop::collection::vec(1i64..500, 1..=50),
        tokens in prop::collection::vec(1u64..5000, 1..=50),
    ) {
        let cost_cap = Money::new("USD", cap_minor);
        let mut budget = Budget::new(
            Some(cost_cap.clone()),
            Some(token_cap),
            Some(Duration::from_secs(3600)), // generous time cap
        );

        let now = chrono::Utc::now();

        for (i, (cost_amount, token_amount)) in costs.iter().zip(tokens.iter()).enumerate() {
            let projected_cost = Money::new("USD", *cost_amount);
            let action = BudgetGuard::check(
                &budget,
                Some(&projected_cost),
                *token_amount,
                now,
                false, // no downshift — test the halt path
            );

            match action {
                BudgetAction::Proceed | BudgetAction::Warn { .. } => {
                    // Authorized — record usage.
                    budget.record_usage(Some(&projected_cost), *token_amount).unwrap();
                }
                BudgetAction::DownShift { .. } | BudgetAction::Halt { .. } => {
                    // Guard prevented the step — stop here.
                    break;
                }
            }

            // INVARIANT: cumulative usage never exceeds the cap.
            if let Some(ref spent) = budget.spent_cost {
                prop_assert!(
                    spent.try_cmp(&cost_cap) != Some(std::cmp::Ordering::Greater),
                    "Cost exceeded cap at step {}: spent={}, cap={}",
                    i, spent.minor_units, cost_cap.minor_units
                );
            }
            prop_assert!(
                budget.spent_tokens <= token_cap,
                "Tokens exceeded cap at step {}: spent={}, cap={}",
                i, budget.spent_tokens, token_cap
            );
        }
    }
}

// ─── Property 10: Escalation determinism ─────────────────────────────────────

/// Represents a step outcome for the router.
#[derive(Debug, Clone, Copy)]
enum StepOutcome {
    Success,
    Failure,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Property 10: Exactly two consecutive failures trigger escalation; a
    /// success after escalation returns selection to the local default.
    ///
    /// **Validates: Requirements 12.2, 12.3**
    #[test]
    fn prop10_escalation_determinism(
        outcomes in prop::collection::vec(
            prop_oneof![Just(StepOutcome::Success), Just(StepOutcome::Failure)],
            1..=30
        )
    ) {
        let mut router = make_router();
        let mut consecutive_failures = 0u32;

        for outcome in &outcomes {
            let was_escalated = router.is_escalated();

            match outcome {
                StepOutcome::Failure => {
                    router.report_failure();
                    consecutive_failures += 1;
                }
                StepOutcome::Success => {
                    router.report_success();
                    consecutive_failures = 0;
                }
            }

            // After exactly 2 consecutive failures on local, must be escalated.
            if consecutive_failures >= 2 && !was_escalated {
                // Should have escalated (if premium is available).
                if !router.is_escalated() {
                    // Only valid if there are no premium providers.
                    // Our test router has premium, so this should not happen.
                    prop_assert!(
                        router.is_escalated(),
                        "Should have escalated after 2 consecutive failures"
                    );
                }
                consecutive_failures = 0; // Router resets on escalation.
            }

            // After a success on premium, must de-escalate to local.
            if matches!(outcome, StepOutcome::Success) {
                prop_assert!(
                    !router.is_escalated(),
                    "Should have de-escalated after success"
                );
            }
        }
    }
}
