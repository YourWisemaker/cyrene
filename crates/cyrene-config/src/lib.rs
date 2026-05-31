//! `cyrene-config`: single-file TOML configuration loading for Cyrene.
//!
//! Cyrene is configured by **one** human-editable TOML file (default
//! `~/.cyrene/config.toml`, R2.5). This crate deserializes that file into a
//! [`Config`] whose provider/channel/memory components are declared in a
//! `[type.alias]` shape (e.g. `[providers.openai.coding]`), plus an
//! [`AutonomyConfig`] section that is **secure-by-default** (medium-risk needs
//! approval, high-risk is blocked, R22.1; loosening it requires an explicit
//! config edit, R22.4).
//!
//! Secrets (API keys, channel tokens) are **never** stored in the TOML. The
//! config references each secret only by the *name* of the environment variable
//! that holds it; the [`SecretResolver`] reads the actual value from the
//! environment, optionally seeded from a `.env` file. A committed
//! `.env.example` at the repo root documents every supported variable with
//! placeholder values; the real `.env` is git-ignored and never committed.
//!
//! The [`PluginRegistry`] turns this [`Config`] into live components: it
//! instantiates each declared Channel, Memory backend, and Model_Provider by
//! its `type`/`alias` via a registered factory, exposes alias-keyed lookup
//! tables, and on a component init failure skips it and surfaces the error to
//! the caller for ledger logging (R2.2, R2.4). Concrete providers/channels live
//! in other crates, so the registry stays decoupled from them through the
//! factory abstraction — they register from the outside with no core change.

mod autonomy;
mod config;
mod error;
mod registry;
mod secrets;

pub use autonomy::{AutonomyAction, AutonomyConfig};
pub use config::{
    AliasMap, ChannelEntry, ComponentRef, Config, MemoryEntry, ProviderEntry, TypeAliasMap,
};
pub use error::ConfigError;
pub use registry::{
    BoxError, BuildContext, ComponentKey, ComponentKind, LoadError, LoadFailure, PluginRegistry,
};
pub use secrets::SecretResolver;

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
