//! Session budget tracking.
//!
//! The [`Budget`] meters cumulative cost, tokens, and time against optional
//! caps. Its helpers compute remaining headroom and, critically, whether a
//! *projected* addition would breach any cap — this is the building block for
//! the Budget_Guard's "usage never exceeds a configured limit" invariant
//! (R13.6), which is checked *before* a step is authorized.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::money::Money;

/// Identifies which configured cap a projection would breach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BudgetLimit {
    /// The monetary cost cap.
    Cost,
    /// The token-count cap.
    Tokens,
    /// The wall-clock time cap.
    Time,
}

/// Per-session limits and the usage accumulated against them.
///
/// Every cap is optional; an absent cap means that dimension is unlimited.
/// `spent_cost` is optional because cost is only meaningful once at least one
/// priced step has run and establishes the session currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// Optional maximum spend for the session.
    pub cost_cap: Option<Money>,
    /// Optional maximum cumulative token count.
    pub token_cap: Option<u64>,
    /// Optional maximum wall-clock duration.
    pub time_cap: Option<Duration>,
    /// Cost accumulated so far (`None` until the first priced step).
    pub spent_cost: Option<Money>,
    /// Tokens consumed so far.
    pub spent_tokens: u64,
    /// When the session (and therefore the time cap) started.
    pub started_at: DateTime<Utc>,
}

impl Budget {
    /// Creates an unlimited budget that started now.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            cost_cap: None,
            token_cap: None,
            time_cap: None,
            spent_cost: None,
            spent_tokens: 0,
            started_at: Utc::now(),
        }
    }

    /// Creates a budget with the given caps, starting now with zero usage.
    #[must_use]
    pub fn new(
        cost_cap: Option<Money>,
        token_cap: Option<u64>,
        time_cap: Option<Duration>,
    ) -> Self {
        Self {
            cost_cap,
            token_cap,
            time_cap,
            spent_cost: None,
            spent_tokens: 0,
            started_at: Utc::now(),
        }
    }

    /// Returns the cost remaining before the cost cap, if a cap is set.
    ///
    /// A `Some(Money)` may be negative if usage already exceeds the cap. Returns
    /// `None` when no cost cap is configured.
    ///
    /// # Errors
    /// Returns [`CoreError::CurrencyMismatch`] if recorded spend uses a
    /// different currency than the cap.
    pub fn remaining_cost(&self) -> Result<Option<Money>, CoreError> {
        let Some(cap) = &self.cost_cap else {
            return Ok(None);
        };
        match &self.spent_cost {
            Some(spent) => Ok(Some(cap.checked_sub(spent)?)),
            None => Ok(Some(cap.clone())),
        }
    }

    /// Returns the tokens remaining before the token cap, saturating at zero.
    /// Returns `None` when no token cap is configured.
    #[must_use]
    pub fn remaining_tokens(&self) -> Option<u64> {
        self.token_cap
            .map(|cap| cap.saturating_sub(self.spent_tokens))
    }

    /// Returns the time remaining before the time cap, given the current
    /// instant. Saturates at zero. Returns `None` when no time cap is set.
    #[must_use]
    pub fn remaining_time(&self, now: DateTime<Utc>) -> Option<Duration> {
        self.time_cap.map(|cap| {
            let elapsed = now
                .signed_duration_since(self.started_at)
                .to_std()
                .unwrap_or(Duration::ZERO);
            cap.saturating_sub(elapsed)
        })
    }

    /// Returns the wall-clock time elapsed since the session started.
    #[must_use]
    pub fn elapsed(&self, now: DateTime<Utc>) -> Duration {
        now.signed_duration_since(self.started_at)
            .to_std()
            .unwrap_or(Duration::ZERO)
    }

    /// Reports whether adding `add_cost` would push spend over the cost cap.
    ///
    /// Returns `Ok(true)` when the projected total strictly exceeds the cap.
    /// With no cap configured the projection can never exceed, so `Ok(false)`.
    ///
    /// # Errors
    /// Returns an error if currencies mismatch or the addition overflows.
    pub fn would_exceed_cost(&self, add_cost: &Money) -> Result<bool, CoreError> {
        let Some(cap) = &self.cost_cap else {
            return Ok(false);
        };
        let projected = match &self.spent_cost {
            Some(spent) => spent.checked_add(add_cost)?,
            None => add_cost.clone(),
        };
        match projected.try_cmp(cap) {
            Some(ordering) => Ok(ordering == core::cmp::Ordering::Greater),
            None => Err(CoreError::CurrencyMismatch {
                expected: cap.currency.clone(),
                found: projected.currency,
            }),
        }
    }

    /// Reports whether consuming `add_tokens` more would exceed the token cap.
    #[must_use]
    pub fn would_exceed_tokens(&self, add_tokens: u64) -> bool {
        match self.token_cap {
            Some(cap) => self.spent_tokens.saturating_add(add_tokens) > cap,
            None => false,
        }
    }

    /// Reports whether the session has exceeded its time cap as of `now`.
    #[must_use]
    pub fn would_exceed_time(&self, now: DateTime<Utc>) -> bool {
        match self.time_cap {
            Some(cap) => self.elapsed(now) > cap,
            None => false,
        }
    }

    /// Reports the first cap (if any) that a projected step would breach.
    ///
    /// The Budget_Guard calls this *before* authorizing a step so it can
    /// down-shift or halt rather than overshoot a limit (R13.4–R13.6).
    ///
    /// # Errors
    /// Returns an error if the projected cost currency mismatches the cap.
    pub fn projected_breach(
        &self,
        add_cost: Option<&Money>,
        add_tokens: u64,
        now: DateTime<Utc>,
    ) -> Result<Option<BudgetLimit>, CoreError> {
        if let Some(cost) = add_cost {
            if self.would_exceed_cost(cost)? {
                return Ok(Some(BudgetLimit::Cost));
            }
        }
        if self.would_exceed_tokens(add_tokens) {
            return Ok(Some(BudgetLimit::Tokens));
        }
        if self.would_exceed_time(now) {
            return Ok(Some(BudgetLimit::Time));
        }
        Ok(None)
    }

    /// Records consumed cost and tokens against the running totals.
    ///
    /// This mutates usage only; it does not enforce caps. Callers should check
    /// [`Budget::projected_breach`] before authorizing the step that produces
    /// this usage.
    ///
    /// # Errors
    /// Returns an error if the recorded cost currency differs from prior spend
    /// or the cost cap, or if accumulation overflows.
    pub fn record_usage(
        &mut self,
        add_cost: Option<&Money>,
        add_tokens: u64,
    ) -> Result<(), CoreError> {
        if let Some(cost) = add_cost {
            let next = match &self.spent_cost {
                Some(spent) => spent.checked_add(cost)?,
                None => cost.clone(),
            };
            self.spent_cost = Some(next);
        }
        self.spent_tokens = self
            .spent_tokens
            .checked_add(add_tokens)
            .ok_or(CoreError::Overflow("Budget::record_usage tokens"))?;
        Ok(())
    }
}

impl Default for Budget {
    /// An unlimited budget starting now.
    fn default() -> Self {
        Self::unlimited()
    }
}

#[cfg(test)]
mod tests {
    use super::{Budget, BudgetLimit};
    use crate::error::CoreError;
    use crate::money::Money;
    use chrono::{DateTime, Utc};
    use std::time::Duration;

    /// A fixed epoch instant so time-based assertions are deterministic.
    fn epoch() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_000_000_000, 0).expect("valid timestamp")
    }

    /// Builds a budget with the given caps anchored at [`epoch`] so elapsed time
    /// is computed against a `now` the test controls, not the wall clock.
    fn budget_at_epoch(
        cost_cap: Option<Money>,
        token_cap: Option<u64>,
        time_cap: Option<Duration>,
    ) -> Budget {
        let mut b = Budget::new(cost_cap, token_cap, time_cap);
        b.started_at = epoch();
        b
    }

    #[test]
    fn unlimited_has_no_caps_and_no_breach() {
        let b = Budget::unlimited();
        assert!(b.cost_cap.is_none());
        assert!(b.token_cap.is_none());
        assert!(b.time_cap.is_none());
        assert_eq!(b.remaining_tokens(), None);
        assert_eq!(b.remaining_cost().unwrap(), None);
        assert_eq!(b.remaining_time(Utc::now()), None);
        // Nothing can breach an unlimited budget.
        assert!(!b.would_exceed_tokens(u64::MAX));
        assert!(!b.would_exceed_cost(&Money::new("USD", i64::MAX)).unwrap());
    }

    #[test]
    fn default_is_unlimited() {
        let b = Budget::default();
        assert!(b.cost_cap.is_none() && b.token_cap.is_none() && b.time_cap.is_none());
    }

    #[test]
    fn record_usage_accumulates_cost_and_tokens() {
        let mut b = Budget::new(None, None, None);
        b.record_usage(Some(&Money::new("USD", 100)), 10).unwrap();
        b.record_usage(Some(&Money::new("USD", 250)), 15).unwrap();
        assert_eq!(b.spent_cost, Some(Money::new("USD", 350)));
        assert_eq!(b.spent_tokens, 25);
    }

    #[test]
    fn record_usage_tokens_only_leaves_cost_none() {
        let mut b = Budget::new(None, None, None);
        b.record_usage(None, 5).unwrap();
        assert_eq!(b.spent_cost, None);
        assert_eq!(b.spent_tokens, 5);
    }

    #[test]
    fn record_usage_currency_mismatch_errors() {
        let mut b = Budget::new(None, None, None);
        b.record_usage(Some(&Money::new("USD", 100)), 0).unwrap();
        assert!(matches!(
            b.record_usage(Some(&Money::new("EUR", 100)), 0),
            Err(CoreError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn record_usage_token_overflow_errors() {
        let mut b = Budget::new(None, None, None);
        b.spent_tokens = u64::MAX;
        assert!(matches!(
            b.record_usage(None, 1),
            Err(CoreError::Overflow(_))
        ));
    }

    #[test]
    fn remaining_cost_reflects_spend() {
        let mut b = Budget::new(Some(Money::new("USD", 1000)), None, None);
        assert_eq!(b.remaining_cost().unwrap(), Some(Money::new("USD", 1000)));
        b.record_usage(Some(&Money::new("USD", 300)), 0).unwrap();
        assert_eq!(b.remaining_cost().unwrap(), Some(Money::new("USD", 700)));
    }

    #[test]
    fn remaining_cost_can_go_negative() {
        let mut b = Budget::new(Some(Money::new("USD", 100)), None, None);
        b.record_usage(Some(&Money::new("USD", 150)), 0).unwrap();
        assert_eq!(b.remaining_cost().unwrap(), Some(Money::new("USD", -50)));
    }

    #[test]
    fn remaining_tokens_saturates_at_zero() {
        let mut b = Budget::new(None, Some(100), None);
        assert_eq!(b.remaining_tokens(), Some(100));
        b.spent_tokens = 70;
        assert_eq!(b.remaining_tokens(), Some(30));
        b.spent_tokens = 250;
        assert_eq!(b.remaining_tokens(), Some(0));
    }

    #[test]
    fn remaining_time_saturates_at_zero() {
        let b = budget_at_epoch(None, None, Some(Duration::from_secs(60)));
        let now = epoch() + chrono::Duration::seconds(20);
        assert_eq!(b.remaining_time(now), Some(Duration::from_secs(40)));
        let later = epoch() + chrono::Duration::seconds(120);
        assert_eq!(b.remaining_time(later), Some(Duration::ZERO));
    }

    #[test]
    fn elapsed_measures_against_now() {
        let b = budget_at_epoch(None, None, None);
        let now = epoch() + chrono::Duration::seconds(42);
        assert_eq!(b.elapsed(now), Duration::from_secs(42));
    }

    // --- The R13.6 boundary invariant: at-cap is allowed, strictly over is a breach.

    #[test]
    fn would_exceed_cost_at_cap_is_allowed_over_is_breach() {
        let b = Budget::new(Some(Money::new("USD", 1000)), None, None);
        // Exactly at the cap: allowed.
        assert!(!b.would_exceed_cost(&Money::new("USD", 1000)).unwrap());
        // One minor unit over: breach.
        assert!(b.would_exceed_cost(&Money::new("USD", 1001)).unwrap());
        // Below the cap: allowed.
        assert!(!b.would_exceed_cost(&Money::new("USD", 999)).unwrap());
    }

    #[test]
    fn would_exceed_cost_accounts_for_prior_spend() {
        let mut b = Budget::new(Some(Money::new("USD", 1000)), None, None);
        b.record_usage(Some(&Money::new("USD", 600)), 0).unwrap();
        // 600 + 400 == 1000 cap exactly: allowed.
        assert!(!b.would_exceed_cost(&Money::new("USD", 400)).unwrap());
        // 600 + 401 == 1001 > cap: breach.
        assert!(b.would_exceed_cost(&Money::new("USD", 401)).unwrap());
    }

    #[test]
    fn would_exceed_cost_currency_mismatch_errors() {
        let b = Budget::new(Some(Money::new("USD", 1000)), None, None);
        assert!(matches!(
            b.would_exceed_cost(&Money::new("EUR", 1)),
            Err(CoreError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn would_exceed_tokens_at_cap_is_allowed_over_is_breach() {
        let mut b = Budget::new(None, Some(1000), None);
        // Exactly at the cap: allowed.
        assert!(!b.would_exceed_tokens(1000));
        // One over: breach.
        assert!(b.would_exceed_tokens(1001));
        b.spent_tokens = 600;
        // 600 + 400 == cap: allowed.
        assert!(!b.would_exceed_tokens(400));
        // 600 + 401 > cap: breach.
        assert!(b.would_exceed_tokens(401));
    }

    #[test]
    fn would_exceed_time_at_cap_is_allowed_over_is_breach() {
        let b = budget_at_epoch(None, None, Some(Duration::from_secs(60)));
        // Exactly at the cap: not exceeded.
        assert!(!b.would_exceed_time(epoch() + chrono::Duration::seconds(60)));
        // One second over: breach.
        assert!(b.would_exceed_time(epoch() + chrono::Duration::seconds(61)));
        // Before the cap: allowed.
        assert!(!b.would_exceed_time(epoch() + chrono::Duration::seconds(30)));
    }

    #[test]
    fn projected_breach_none_when_within_all_limits() {
        let b = budget_at_epoch(
            Some(Money::new("USD", 1000)),
            Some(1000),
            Some(Duration::from_secs(60)),
        );
        let now = epoch() + chrono::Duration::seconds(10);
        let breach = b
            .projected_breach(Some(&Money::new("USD", 500)), 500, now)
            .unwrap();
        assert_eq!(breach, None);
    }

    #[test]
    fn projected_breach_at_every_cap_boundary_is_none() {
        // Hitting each cap exactly must NOT report a breach (R13.6 invariant).
        let b = budget_at_epoch(
            Some(Money::new("USD", 1000)),
            Some(1000),
            Some(Duration::from_secs(60)),
        );
        let now = epoch() + chrono::Duration::seconds(60);
        let breach = b
            .projected_breach(Some(&Money::new("USD", 1000)), 1000, now)
            .unwrap();
        assert_eq!(breach, None);
    }

    #[test]
    fn projected_breach_reports_cost_first() {
        let b = budget_at_epoch(Some(Money::new("USD", 1000)), Some(1000), None);
        let breach = b
            .projected_breach(Some(&Money::new("USD", 1001)), 0, epoch())
            .unwrap();
        assert_eq!(breach, Some(BudgetLimit::Cost));
    }

    #[test]
    fn projected_breach_reports_tokens() {
        let b = budget_at_epoch(Some(Money::new("USD", 1000)), Some(1000), None);
        let breach = b
            .projected_breach(Some(&Money::new("USD", 10)), 1001, epoch())
            .unwrap();
        assert_eq!(breach, Some(BudgetLimit::Tokens));
    }

    #[test]
    fn projected_breach_reports_time() {
        let b = budget_at_epoch(None, None, Some(Duration::from_secs(60)));
        let now = epoch() + chrono::Duration::seconds(61);
        let breach = b.projected_breach(None, 0, now).unwrap();
        assert_eq!(breach, Some(BudgetLimit::Time));
    }

    #[test]
    fn projected_breach_currency_mismatch_errors() {
        let b = budget_at_epoch(Some(Money::new("USD", 1000)), None, None);
        assert!(matches!(
            b.projected_breach(Some(&Money::new("EUR", 10)), 0, epoch()),
            Err(CoreError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn projected_breach_is_checked_before_record_usage_mutates() {
        // The "check before authorize" property R13.6 builds on: a guard asks
        // projected_breach *before* recording usage, so a step that would
        // overshoot the cap is caught while usage still reflects the prior,
        // within-budget state.
        let mut b = budget_at_epoch(Some(Money::new("USD", 1000)), Some(1000), None);
        b.record_usage(Some(&Money::new("USD", 900)), 900).unwrap();

        let next_cost = Money::new("USD", 200); // 900 + 200 == 1100 > 1000 cap.
        let next_tokens = 50; // 900 + 50 == 950, within the token cap.

        // The breach is detected for the cost dimension...
        assert_eq!(
            b.projected_breach(Some(&next_cost), next_tokens, epoch())
                .unwrap(),
            Some(BudgetLimit::Cost)
        );
        // ...and crucially, the check did not mutate accumulated usage, so the
        // guard can safely withhold the step without having spent the budget.
        assert_eq!(b.spent_cost, Some(Money::new("USD", 900)));
        assert_eq!(b.spent_tokens, 900);

        // A within-budget projection reports no breach, after which recording
        // the usage advances the totals.
        let ok_cost = Money::new("USD", 100); // 900 + 100 == 1000 cap exactly.
        assert_eq!(
            b.projected_breach(Some(&ok_cost), next_tokens, epoch())
                .unwrap(),
            None
        );
        b.record_usage(Some(&ok_cost), next_tokens).unwrap();
        assert_eq!(b.spent_cost, Some(Money::new("USD", 1000)));
        assert_eq!(b.spent_tokens, 950);
    }

    #[test]
    fn serde_round_trip() {
        let b = budget_at_epoch(
            Some(Money::new("USD", 5000)),
            Some(100_000),
            Some(Duration::from_secs(900)),
        );
        let json = serde_json::to_string(&b).unwrap();
        let back: Budget = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }
}
