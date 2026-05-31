//! Currency amounts.
//!
//! [`Money`] stores an amount as a signed integer count of **minor units**
//! (e.g. cents for USD) together with an ISO-4217-style currency code. Integer
//! storage avoids the rounding errors inherent in binary floating point, which
//! matters because [`Money`] feeds the Budget_Guard's "never exceed" invariant
//! (R13.6).

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// A monetary amount expressed in integer minor units of a currency.
///
/// `minor_units` is the smallest indivisible unit of the currency (cents,
/// pence, etc.). For example `Money { currency: "USD", minor_units: 150 }`
/// represents `$1.50`. Storing money this way keeps arithmetic exact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    /// ISO-4217-style currency code, e.g. `"USD"`.
    pub currency: String,
    /// Amount in the currency's smallest unit. May be negative.
    pub minor_units: i64,
}

impl Money {
    /// Creates an amount in the given currency's minor units.
    pub fn new(currency: impl Into<String>, minor_units: i64) -> Self {
        Self {
            currency: currency.into(),
            minor_units,
        }
    }

    /// Creates a zero amount in the given currency.
    pub fn zero(currency: impl Into<String>) -> Self {
        Self::new(currency, 0)
    }

    /// Returns `true` if the amount is exactly zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.minor_units == 0
    }

    /// Returns `true` if both amounts use the same currency code.
    #[must_use]
    pub fn same_currency(&self, other: &Self) -> bool {
        self.currency == other.currency
    }

    /// Adds two amounts of the same currency.
    ///
    /// # Errors
    /// Returns [`CoreError::CurrencyMismatch`] if the currencies differ, or
    /// [`CoreError::Overflow`] if the addition overflows `i64`.
    pub fn checked_add(&self, other: &Self) -> Result<Self, CoreError> {
        self.ensure_same_currency(other)?;
        let minor_units = self
            .minor_units
            .checked_add(other.minor_units)
            .ok_or(CoreError::Overflow("Money::checked_add"))?;
        Ok(Self::new(self.currency.clone(), minor_units))
    }

    /// Subtracts `other` from `self` (same currency).
    ///
    /// # Errors
    /// Returns [`CoreError::CurrencyMismatch`] if the currencies differ, or
    /// [`CoreError::Overflow`] if the subtraction overflows `i64`.
    pub fn checked_sub(&self, other: &Self) -> Result<Self, CoreError> {
        self.ensure_same_currency(other)?;
        let minor_units = self
            .minor_units
            .checked_sub(other.minor_units)
            .ok_or(CoreError::Overflow("Money::checked_sub"))?;
        Ok(Self::new(self.currency.clone(), minor_units))
    }

    /// Compares two amounts of the same currency.
    ///
    /// Returns [`None`] when the currencies differ, since cross-currency
    /// amounts are not comparable without a conversion rate.
    #[must_use]
    pub fn try_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        if self.same_currency(other) {
            Some(self.minor_units.cmp(&other.minor_units))
        } else {
            None
        }
    }

    fn ensure_same_currency(&self, other: &Self) -> Result<(), CoreError> {
        if self.same_currency(other) {
            Ok(())
        } else {
            Err(CoreError::CurrencyMismatch {
                expected: self.currency.clone(),
                found: other.currency.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Money;
    use crate::error::CoreError;
    use core::cmp::Ordering;

    #[test]
    fn new_and_accessors() {
        let m = Money::new("USD", 150);
        assert_eq!(m.currency, "USD");
        assert_eq!(m.minor_units, 150);
    }

    #[test]
    fn zero_and_is_zero() {
        let z = Money::zero("USD");
        assert!(z.is_zero());
        assert_eq!(z.minor_units, 0);
        assert!(!Money::new("USD", 1).is_zero());
        // A negative amount is not zero either.
        assert!(!Money::new("USD", -1).is_zero());
    }

    #[test]
    fn same_currency_checks_code() {
        assert!(Money::new("USD", 1).same_currency(&Money::new("USD", 2)));
        assert!(!Money::new("USD", 1).same_currency(&Money::new("EUR", 1)));
    }

    #[test]
    fn checked_add_same_currency() {
        let a = Money::new("USD", 150);
        let b = Money::new("USD", 250);
        assert_eq!(a.checked_add(&b).unwrap(), Money::new("USD", 400));
    }

    #[test]
    fn checked_sub_same_currency_allows_negative() {
        let a = Money::new("USD", 100);
        let b = Money::new("USD", 250);
        assert_eq!(a.checked_sub(&b).unwrap(), Money::new("USD", -150));
    }

    #[test]
    fn checked_add_currency_mismatch_errors() {
        let a = Money::new("USD", 100);
        let b = Money::new("EUR", 100);
        match a.checked_add(&b) {
            Err(CoreError::CurrencyMismatch { expected, found }) => {
                assert_eq!(expected, "USD");
                assert_eq!(found, "EUR");
            }
            other => panic!("expected CurrencyMismatch, got {other:?}"),
        }
    }

    #[test]
    fn checked_sub_currency_mismatch_errors() {
        let a = Money::new("USD", 100);
        let b = Money::new("GBP", 50);
        assert!(matches!(
            a.checked_sub(&b),
            Err(CoreError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn checked_add_overflow_errors() {
        let a = Money::new("USD", i64::MAX);
        let b = Money::new("USD", 1);
        assert!(matches!(a.checked_add(&b), Err(CoreError::Overflow(_))));
    }

    #[test]
    fn checked_sub_overflow_errors() {
        let a = Money::new("USD", i64::MIN);
        let b = Money::new("USD", 1);
        assert!(matches!(a.checked_sub(&b), Err(CoreError::Overflow(_))));
    }

    #[test]
    fn try_cmp_within_currency() {
        let a = Money::new("USD", 100);
        let b = Money::new("USD", 250);
        assert_eq!(a.try_cmp(&b), Some(Ordering::Less));
        assert_eq!(b.try_cmp(&a), Some(Ordering::Greater));
        assert_eq!(a.try_cmp(&Money::new("USD", 100)), Some(Ordering::Equal));
    }

    #[test]
    fn try_cmp_across_currency_is_none() {
        let a = Money::new("USD", 100);
        let b = Money::new("EUR", 100);
        assert_eq!(a.try_cmp(&b), None);
    }

    #[test]
    fn serde_round_trip() {
        let m = Money::new("USD", -4242);
        let json = serde_json::to_string(&m).unwrap();
        let back: Money = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }
}
