//! The result type for [`Ledger::verify`](crate::Ledger::verify).
//!
//! Verifying the ledger walks every receipt in `seq` order and, for each,
//! checks three invariants in turn (design section 2, R5.3):
//!
//! 1. the stored `hash` recomputes from the receipt's content
//!    ([`compute_hash`](crate::receipt::compute_hash));
//! 2. the chain link holds — `prev_hash` equals the previous receipt's stored
//!    `hash` (or [`ReceiptHash::ZERO`](crate::ReceiptHash::ZERO) for the
//!    genesis receipt at `seq` 0);
//! 3. the `signature` validates against the install verifying key
//!    ([`InstallKey::verify_signature`](crate::InstallKey::verify_signature)).
//!
//! The walk stops at and reports the **first** receipt that fails any check,
//! localizing the divergence by its `seq` and the [`DivergenceKind`] that
//! failed.

/// The outcome of verifying the integrity of the whole Receipt_Ledger (R5.3).
///
/// Either every receipt passes all three checks ([`Valid`](Self::Valid)), or
/// the first failing receipt is reported via [`Diverged`](Self::Diverged) with
/// its `seq` and the [`DivergenceKind`] that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerVerification {
    /// Every receipt's hash recomputes, every chain link holds, and every
    /// signature validates.
    Valid,
    /// The ledger diverges at the first receipt that fails a check; later
    /// receipts are not reported.
    Diverged {
        /// The `seq` of the first receipt that failed verification.
        seq: u64,
        /// Which check failed at `seq`.
        kind: DivergenceKind,
    },
}

impl LedgerVerification {
    /// Builds a [`Diverged`](Self::Diverged) outcome for `seq`/`kind`.
    #[must_use]
    pub(crate) const fn diverged(seq: u64, kind: DivergenceKind) -> Self {
        Self::Diverged { seq, kind }
    }

    /// Returns `true` when the ledger verified cleanly with no divergence.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    /// Returns the `seq` of the first diverging receipt, or [`None`] when the
    /// ledger is [`Valid`](Self::Valid).
    #[must_use]
    pub const fn divergence_seq(&self) -> Option<u64> {
        match self {
            Self::Valid => None,
            Self::Diverged { seq, .. } => Some(*seq),
        }
    }
}

/// The kind of check a receipt failed during [`verify`](crate::Ledger::verify).
///
/// Checks are applied in this order, and the first to fail is the one reported,
/// so the variants are listed in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergenceKind {
    /// The recomputed `hash` does not equal the stored `hash`: the receipt's
    /// own content (`seq`, `timestamp`, `action`, `inputs_digest`,
    /// `deciding_model`, `prev_hash`) was altered, or the stored `hash` itself
    /// was altered.
    HashMismatch,
    /// The receipt's `prev_hash` does not equal the previous receipt's stored
    /// `hash` (or the genesis [`ReceiptHash::ZERO`](crate::ReceiptHash::ZERO)
    /// for `seq` 0): the chain link is broken, e.g. a receipt was removed or
    /// reordered.
    BrokenLink,
    /// The stored `signature` does not validate against the install verifying
    /// key for the stored `hash`: the signature was altered or was not produced
    /// by the install key.
    BadSignature,
}

#[cfg(test)]
mod tests {
    use super::{DivergenceKind, LedgerVerification};

    #[test]
    fn valid_is_valid_and_has_no_divergence_seq() {
        let v = LedgerVerification::Valid;
        assert!(v.is_valid());
        assert_eq!(v.divergence_seq(), None);
    }

    #[test]
    fn diverged_reports_seq_and_is_not_valid() {
        let v = LedgerVerification::diverged(7, DivergenceKind::BadSignature);
        assert!(!v.is_valid());
        assert_eq!(v.divergence_seq(), Some(7));
        assert_eq!(
            v,
            LedgerVerification::Diverged {
                seq: 7,
                kind: DivergenceKind::BadSignature,
            }
        );
    }
}
