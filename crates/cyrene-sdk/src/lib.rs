//! `cyrene-sdk`: the Extension SDK for Cyrene (R31).
//!
//! This crate re-exports the stable [`Channel`], [`Memory`], and [`Model`]
//! traits, the [`Tool`] interface, and a [`HostApi`] so that an Extension
//! builds without depending on internal crates (R31.1). Extensions depend only
//! on `cyrene-sdk`; the SDK is the versioned contract.
//!
//! The [`ExtensionManifest`] (`cyrene.plugin.toml`) schema (R31.2) declares
//! each extension's name, version, capabilities, requested permissions, and
//! host compatibility range.

mod error;
mod host_api;
mod manifest;

pub use cyrene_core::{
    Channel, ChannelError, ChannelHealth, ChannelId, ChatMessage, Fact, FinishReason,
    InboundMessage, Memory, MemoryError, MemoryHit, MemoryQuery, Model, ModelDescriptor,
    ModelError, ModelRequest, ModelResponse, OutboundMessage, Relation, Risk, Role, Tier,
    TokenUsage,
};
pub use cyrene_tools::{Tool, ToolError, ToolInvocation, ToolOutput, ToolRegistry};
pub use error::SdkError;
pub use host_api::HostApi;
pub use manifest::{Capability, ExtensionManifest, Permissions};

/// The current host SDK version, read from the package version.
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-sdk"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
