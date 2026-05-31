//! Error model for `cyrene-core`.
//!
//! Per the design's "Error Handling" section, each crate defines a `thiserror`
//! enum and the binary aggregates with `anyhow`. Every error carries a
//! [`Recoverability`] hint so the Agent_Loop can decide how to react
//! (`Retry | Escalate | Halt | UserAction`).

use serde::{Deserialize, Serialize};

/// How the runtime should react to an error.
///
/// This hint lets the Agent_Loop and Model_Router treat failures uniformly:
/// transient failures can be retried, provider failures can be escalated,
/// unrecoverable failures halt the plan, and policy failures ask the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Recoverability {
    /// The operation may succeed if retried as-is.
    Retry,
    /// Escalate to a more capable (premium) provider or path.
    Escalate,
    /// Stop the current plan; the error cannot be recovered automatically.
    Halt,
    /// Defer to the user (e.g. approval, corrective input, or configuration).
    UserAction,
}

/// A contract for associating a [`Recoverability`] hint with an error type.
///
/// Other crates implement this on their own `thiserror` enums so the loop can
/// classify any error without knowing its concrete type.
pub trait Recoverable {
    /// Returns the recommended recovery strategy for this error.
    fn recoverability(&self) -> Recoverability;
}

/// The crate-level error type for `cyrene-core`.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// Two monetary amounts (or a budget and an amount) used different
    /// currencies, so the operation is undefined.
    #[error("currency mismatch: expected `{expected}`, found `{found}`")]
    CurrencyMismatch {
        /// The currency that was expected (the left-hand side).
        expected: String,
        /// The currency that was supplied (the right-hand side).
        found: String,
    },

    /// A configured budget cap was, or would be, exceeded.
    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    /// An arithmetic operation overflowed its integer representation.
    #[error("arithmetic overflow in {0}")]
    Overflow(&'static str),

    /// A value failed to serialize or deserialize.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl Recoverable for CoreError {
    fn recoverability(&self) -> Recoverability {
        match self {
            Self::CurrencyMismatch { .. }
            | Self::BudgetExceeded(_)
            | Self::Overflow(_)
            | Self::Serialization(_) => Recoverability::Halt,
        }
    }
}
