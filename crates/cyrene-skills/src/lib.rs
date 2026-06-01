//! `cyrene-skills`: the Skill_Engine, Skill_Library, and skill bundles (R14, R15).
//!
//! Cyrene gets faster and more capable over time by turning tasks it solves
//! into reusable [`Skill`]s. This crate implements the closed loop:
//!
//! - [`Skill`] — the unit of capability, persisted in the `SKILL.md` format
//!   (front-matter metadata + Markdown instructions + optional code).
//! - [`SkillLibrary`] — filesystem-backed durable storage of saved skills.
//! - [`SkillEngine`] — the create→test→confirm→save lifecycle (R14), skill
//!   improvement-updates (R14.6), and [`SkillBundle`] confirmation (R15).
//!
//! The engine's sandbox, ledger, and user-confirmation dependencies are
//! expressed as the [`SandboxTester`], [`SkillLedger`], and [`Confirmer`]
//! traits so the loop is testable in isolation and the crate stays decoupled
//! from the concrete safety/ledger/channel crates.

mod bundle;
mod engine;
mod error;
mod library;
mod skill;

pub use bundle::SkillBundle;
pub use engine::{Confirmer, SandboxTester, SkillDecision, SkillEngine, SkillLedger, TestOutcome};
pub use error::SkillError;
pub use library::{SkillLibrary, SkillSummary};
pub use skill::{Skill, SkillId};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-skills"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
