//! `cyrene-state`: the git-style State_Tree and checkpoint/rollback store for Cyrene.
//!
//! Placeholder scaffold (task 1). The real implementation lands in a later task;
//! for now the crate exposes only a subsystem identifier so the workspace
//! compiles and `cargo test` has something to run.

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-state"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
