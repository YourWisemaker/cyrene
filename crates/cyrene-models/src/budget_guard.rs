//! Budget Guard: enforces cost/token/time limits per session (R13).

use chrono::{DateTime, Utc};
use cyrene_core::{Budget, BudgetLimit, Money};

/// Actions the Budget Guard recommends when a limit is approaching or breached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetAction {
    /// Proceed normally — well within budget.
    Proceed,
    /// Warn the user: usage has reached 80% of a configured limit.
    Warn { limit: BudgetLimit, message: String },
    /// Down-shift to a cheaper model to stay within budget.
    DownShift { limit: BudgetLimit, message: String },
    /// Halt the session: no cheaper option fits within the remaining budget.
    Halt { limit: BudgetLimit, message: String },
}

/// The Budget Guard: checks projected usage against session limits.
pub struct BudgetGuard;

impl BudgetGuard {
    /// Check whether a projected step would breach the budget, and recommend
    /// an action. This is called BEFORE authorizing a step (R13.4-R13.6).
    ///
    /// - If projected usage would exceed a limit and no down-shift is possible, returns Halt.
    /// - If usage is at 80%+ of a limit, returns Warn.
    /// - Otherwise returns Proceed.
    pub fn check(
        budget: &Budget,
        projected_cost: Option<&Money>,
        projected_tokens: u64,
        now: DateTime<Utc>,
        can_downshift: bool,
    ) -> BudgetAction {
        // Check time first (can't be fixed by down-shifting).
        if budget.would_exceed_time(now) {
            return BudgetAction::Halt {
                limit: BudgetLimit::Time,
                message: "session time limit exceeded".to_owned(),
            };
        }

        // Check cost breach.
        if let Some(cost) = projected_cost {
            if let Ok(true) = budget.would_exceed_cost(cost) {
                if can_downshift {
                    return BudgetAction::DownShift {
                        limit: BudgetLimit::Cost,
                        message: "projected cost would exceed budget; switching to cheaper model"
                            .to_owned(),
                    };
                }
                return BudgetAction::Halt {
                    limit: BudgetLimit::Cost,
                    message: "projected cost would exceed budget and no cheaper option available"
                        .to_owned(),
                };
            }
        }

        // Check token breach.
        if budget.would_exceed_tokens(projected_tokens) {
            if can_downshift {
                return BudgetAction::DownShift {
                    limit: BudgetLimit::Tokens,
                    message: "projected tokens would exceed budget; trimming context".to_owned(),
                };
            }
            return BudgetAction::Halt {
                limit: BudgetLimit::Tokens,
                message: "projected tokens would exceed budget and no reduction possible"
                    .to_owned(),
            };
        }

        // Check 80% warnings.
        if let Some(remaining) = budget.remaining_tokens() {
            if let Some(cap) = budget.token_cap {
                if remaining <= cap / 5 {
                    // 80% used (remaining <= 20% of cap)
                    return BudgetAction::Warn {
                        limit: BudgetLimit::Tokens,
                        message: format!("token usage at 80%+: {} remaining of {}", remaining, cap),
                    };
                }
            }
        }

        if let Ok(Some(remaining_cost)) = budget.remaining_cost() {
            if let Some(cap) = &budget.cost_cap {
                // Check if remaining is <= 20% of cap
                if let Some(std::cmp::Ordering::Greater) =
                    cap.checked_sub(&remaining_cost).ok().and_then(|used| {
                        // used > 80% of cap means remaining < 20% of cap
                        let threshold = Money::new(cap.currency.clone(), cap.minor_units * 4 / 5);
                        used.try_cmp(&threshold)
                    })
                {
                    return BudgetAction::Warn {
                        limit: BudgetLimit::Cost,
                        message: format!(
                            "cost usage at 80%+: {} remaining",
                            remaining_cost.minor_units
                        ),
                    };
                }
            }
        }

        if let Some(remaining_time) = budget.remaining_time(now) {
            if let Some(cap) = budget.time_cap {
                if remaining_time <= cap / 5 {
                    return BudgetAction::Warn {
                        limit: BudgetLimit::Time,
                        message: format!("time usage at 80%+: {:?} remaining", remaining_time),
                    };
                }
            }
        }

        BudgetAction::Proceed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn within_budget_proceeds() {
        let budget = Budget::new(
            Some(Money::new("USD", 1000)),
            Some(100_000),
            Some(Duration::from_secs(3600)),
        );
        let action =
            BudgetGuard::check(&budget, Some(&Money::new("USD", 10)), 100, Utc::now(), true);
        assert_eq!(action, BudgetAction::Proceed);
    }

    #[test]
    fn cost_breach_with_downshift_available() {
        let budget = Budget::new(Some(Money::new("USD", 100)), None, None);
        let action =
            BudgetGuard::check(&budget, Some(&Money::new("USD", 200)), 0, Utc::now(), true);
        assert!(matches!(
            action,
            BudgetAction::DownShift {
                limit: BudgetLimit::Cost,
                ..
            }
        ));
    }

    #[test]
    fn cost_breach_without_downshift_halts() {
        let budget = Budget::new(Some(Money::new("USD", 100)), None, None);
        let action =
            BudgetGuard::check(&budget, Some(&Money::new("USD", 200)), 0, Utc::now(), false);
        assert!(matches!(
            action,
            BudgetAction::Halt {
                limit: BudgetLimit::Cost,
                ..
            }
        ));
    }

    #[test]
    fn token_breach_halts_when_no_downshift() {
        let budget = Budget::new(None, Some(1000), None);
        let action = BudgetGuard::check(&budget, None, 2000, Utc::now(), false);
        assert!(matches!(
            action,
            BudgetAction::Halt {
                limit: BudgetLimit::Tokens,
                ..
            }
        ));
    }

    #[test]
    fn token_breach_downshifts_when_available() {
        let budget = Budget::new(None, Some(1000), None);
        let action = BudgetGuard::check(&budget, None, 2000, Utc::now(), true);
        assert!(matches!(
            action,
            BudgetAction::DownShift {
                limit: BudgetLimit::Tokens,
                ..
            }
        ));
    }

    #[test]
    fn time_exceeded_halts() {
        let budget = Budget::new(None, None, Some(Duration::from_secs(1)));
        // Simulate time passing by using a future timestamp.
        let future = Utc::now() + chrono::Duration::seconds(10);
        let action = BudgetGuard::check(&budget, None, 0, future, true);
        assert!(matches!(
            action,
            BudgetAction::Halt {
                limit: BudgetLimit::Time,
                ..
            }
        ));
    }

    #[test]
    fn warns_at_80_percent_tokens() {
        let mut budget = Budget::new(None, Some(100), None);
        // Spend 85 tokens (85% used, 15 remaining < 20% of 100)
        budget.record_usage(None, 85).unwrap();
        let action = BudgetGuard::check(&budget, None, 5, Utc::now(), true);
        assert!(matches!(
            action,
            BudgetAction::Warn {
                limit: BudgetLimit::Tokens,
                ..
            }
        ));
    }

    #[test]
    fn no_caps_always_proceeds() {
        let budget = Budget::unlimited();
        let action = BudgetGuard::check(
            &budget,
            Some(&Money::new("USD", 99999)),
            99999,
            Utc::now(),
            false,
        );
        assert_eq!(action, BudgetAction::Proceed);
    }
}
