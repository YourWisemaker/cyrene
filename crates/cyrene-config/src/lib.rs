//! `cyrene-config`: TOML configuration loading and the plugin registry for Cyrene.
//!
//! Placeholder scaffold (task 1). Config parsing and the `Plugin_Registry` are
//! implemented in task 3.

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-config"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
