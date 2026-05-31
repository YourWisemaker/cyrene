//! `cyrene-core`: core domain types, traits, and the Agent_Loop contract for Cyrene.
//!
//! This crate is a placeholder scaffold (task 1). Its real contents — `Session`,
//! `Plan`, `Step`, the `Channel`/`Memory`/`Model` traits, and error types — are
//! implemented in later tasks. For now it exposes only a subsystem identifier so
//! the workspace compiles and `cargo test` has something to run.

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-core"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
