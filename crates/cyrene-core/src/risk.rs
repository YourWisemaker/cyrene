//! Step risk classification.

use serde::{Deserialize, Serialize};

/// The risk level the autonomy policy assigns to a [`Step`](crate::Step).
///
/// The variants are ordered `Low < Medium < High`. The derived [`PartialOrd`]
/// and [`Ord`] follow declaration order, so this ordering is intentional and
/// lets the autonomy policy compare a step's risk against a threshold
/// (defaults: low = auto, medium = approval, high = blocked, per R22.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Risk {
    /// Low risk: safe to execute automatically.
    Low,
    /// Medium risk: requires user approval by default.
    Medium,
    /// High risk: blocked by default until autonomy is raised.
    High,
}

impl Default for Risk {
    /// Defaults to the safest classification.
    fn default() -> Self {
        Self::Low
    }
}

#[cfg(test)]
mod tests {
    use super::Risk;

    #[test]
    fn variants_are_ordered_low_medium_high() {
        assert!(Risk::Low < Risk::Medium);
        assert!(Risk::Medium < Risk::High);
        assert!(Risk::Low < Risk::High);
    }

    #[test]
    fn default_is_low() {
        assert_eq!(Risk::default(), Risk::Low);
    }

    #[test]
    fn sorting_orders_by_severity() {
        let mut risks = vec![Risk::High, Risk::Low, Risk::Medium, Risk::Low];
        risks.sort();
        assert_eq!(risks, vec![Risk::Low, Risk::Low, Risk::Medium, Risk::High]);
    }

    #[test]
    fn comparison_against_threshold() {
        // The autonomy policy compares a step's risk against a threshold;
        // anything at or below the threshold is auto-eligible.
        let threshold = Risk::Medium;
        assert!(Risk::Low <= threshold);
        assert!(Risk::Medium <= threshold);
        assert!(Risk::High > threshold);
    }

    #[test]
    fn max_and_min_follow_ordering() {
        assert_eq!(Risk::Low.max(Risk::High), Risk::High);
        assert_eq!(Risk::High.min(Risk::Medium), Risk::Medium);
    }

    #[test]
    fn serde_round_trip_preserves_each_variant() {
        for risk in [Risk::Low, Risk::Medium, Risk::High] {
            let json = serde_json::to_string(&risk).unwrap();
            let back: Risk = serde_json::from_str(&json).unwrap();
            assert_eq!(risk, back);
        }
    }
}
