//! `cyrene-presence`: the Presence_Engine and Persona_Engine (R17, R18).
//!
//! - [`PresenceEngine`] — emits real-time thinking signals and status updates
//!   during long-running work (R17). Tasks running >5s get periodic updates at
//!   ≤30s intervals, plus a completion summary.
//! - [`PersonaEngine`] — adapts communication tone, verbosity, and urgency to
//!   context (R18). Emergency → crisp; creative → relaxed; away → wrap-up.

mod persona;
mod presence;

pub use persona::{Persona, PersonaContext, PersonaEngine};
pub use presence::{PresenceEngine, StatusUpdate};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-presence"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
