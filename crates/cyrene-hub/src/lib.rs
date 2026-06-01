//! `cyrene-hub`: the Skills_Hub client (R25).
//!
//! Lets users publish, search, and install community skills:
//!
//! - [`HubPackage`] — a versioned, ed25519-signed unit wrapping one or more
//!   [`cyrene_skills::Skill`]s with metadata (R25.1).
//! - [`HubClient`] — publish (sign + upload), keyword search (R25.2), install
//!   with signature verification + sandbox validation (R25.3, R25.4), and
//!   version-update detection (R25.5). Publish/install actions are recorded
//!   through a [`HubLedger`] (R25.6).
//!
//! The registry transport ([`Registry`]) and ledger ([`HubLedger`]) are traits
//! so the HTTP backend and `cyrene-ledger` wiring plug in at the CLI layer
//! while the client logic stays testable in isolation.

mod client;
mod package;

pub use client::{HubClient, HubError, HubLedger, Registry, SearchEntry, SkillValidator};
pub use package::{HubPackage, PackageManifest, Version};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-hub"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
