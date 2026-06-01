//! `cyrene-bridge`: the Workspace_Bridge connecting browser, terminal, and
//! cloud repositories into one coordinated context (R8).
//!
//! The bridge lets Cyrene read a symptom in one context (e.g. a browser console
//! error) and apply the fix in another (a local file or a cloud repo), while
//! enforcing the configured [`WorkspaceBoundary`] on every filesystem-targeting
//! action and recording cross-context actions and denied access for the ledger.

mod boundary;
mod bridge;
mod error;

pub use boundary::WorkspaceBoundary;
pub use bridge::{ConsoleLine, Context, CrossContextAction, CrossContextLog, WorkspaceBridge};
pub use error::BridgeError;

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-bridge"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
