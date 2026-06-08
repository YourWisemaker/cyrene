//! External service integrations exposed as [`Tool`](crate::Tool)s (R28).
//!
//! Each integration (Shopify, ElevenLabs speech-to-text, crypto wallets,
//! marketplaces, …) is a [`Tool`] the Agent_Loop can invoke. Like the built-in
//! suite, these tools **validate their arguments and model the requested
//! action**; the actual side-effecting network I/O is performed by the runtime
//! behind the same trait, so the autonomy policy, Approval_Gate, and sandbox
//! boundary apply uniformly (R28.4) and the tools stay unit-testable without a
//! live network or real credentials.
//!
//! Integrations are configuration- and secret-driven: an integration is
//! declared under `[tools.<type>.<alias>]` in the config, its credential is
//! referenced by env-var name (resolved to [`ToolSettings::api_key`]), and any
//! non-secret options (shop domain, chain RPC URL, marketplace base URL) ride
//! in [`ToolSettings::options`]. New integrations register with no core change.

mod marketplace;
mod shopify;
mod stt;
mod twitch;
mod wallet;
mod youtube;

pub use marketplace::MarketplaceTool;
pub use shopify::ShopifyTool;
pub use stt::SpeechToTextTool;
pub use twitch::TwitchTool;
pub use wallet::WalletTool;
pub use youtube::YouTubeTool;

use std::collections::BTreeMap;

use crate::error::ToolError;
use crate::tool::Tool;

/// Non-secret settings plus a resolved credential for one configured tool.
///
/// This mirrors the role of `ProviderEntry`/`SecretResolver` for models, but is
/// a plain DTO so `cyrene-tools` need not depend on `cyrene-config`. The caller
/// (which owns the config + secret resolver) resolves `api_key_env` to a value
/// and fills `options` from the config entry before calling [`create_tool`].
#[derive(Debug, Clone, Default)]
pub struct ToolSettings {
    /// The resolved API credential, if the integration needs one.
    pub api_key: Option<String>,
    /// An optional base-URL override (regional endpoints, self-hosted, proxies).
    pub base_url: Option<String>,
    /// Arbitrary non-secret options (e.g. `shop`, `rpc_url`, `network`).
    pub options: BTreeMap<String, String>,
}

impl ToolSettings {
    /// Returns the value of option `key`, if present.
    #[must_use]
    pub fn option(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(String::as_str)
    }

    /// Returns the resolved API key, erroring if the integration requires one.
    ///
    /// # Errors
    /// Returns [`ToolError::InvalidArgs`] if no credential was resolved.
    pub fn require_api_key(&self, tool: &str) -> Result<&str, ToolError> {
        self.api_key.as_deref().ok_or_else(|| {
            ToolError::InvalidArgs(format!("{tool} requires an API key (set api_key_env)"))
        })
    }

    /// Returns the value of a required option, erroring if it is absent.
    ///
    /// # Errors
    /// Returns [`ToolError::InvalidArgs`] if the option is missing.
    pub fn require_option(&self, tool: &str, key: &str) -> Result<&str, ToolError> {
        self.option(key)
            .ok_or_else(|| ToolError::InvalidArgs(format!("{tool} requires option `{key}`")))
    }
}

/// Builds an integration tool by its config `type` (e.g. `"shopify"`).
///
/// Mirrors `cyrene_models::create_provider`: maps a configured `type` to a
/// concrete [`Tool`]. The returned tool is ready to register in a
/// [`ToolRegistry`](crate::ToolRegistry).
///
/// # Errors
/// Returns [`ToolError::NotFound`] for an unknown `type`, or the tool's own
/// construction error (e.g. a missing required credential/option).
pub fn create_tool(
    type_name: &str,
    alias: &str,
    settings: &ToolSettings,
) -> Result<Box<dyn Tool>, ToolError> {
    match type_name {
        "shopify" => Ok(Box::new(ShopifyTool::new(alias, settings)?)),
        "elevenlabs" | "stt" => Ok(Box::new(SpeechToTextTool::new(alias, settings)?)),
        "wallet" => Ok(Box::new(WalletTool::new(alias, settings)?)),
        "marketplace" => Ok(Box::new(MarketplaceTool::new(alias, settings)?)),
        "youtube" => Ok(Box::new(YouTubeTool::new(alias, settings)?)),
        "twitch" => Ok(Box::new(TwitchTool::new(alias, settings)?)),
        other => Err(ToolError::NotFound(format!(
            "unknown integration tool type: `{other}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with_key() -> ToolSettings {
        ToolSettings {
            api_key: Some("secret-token".to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn create_tool_unknown_type_errors() {
        assert!(matches!(
            create_tool("nope", "x", &ToolSettings::default()),
            Err(ToolError::NotFound(_))
        ));
    }

    #[test]
    fn elevenlabs_and_stt_aliases_build_same_tool() {
        let a = create_tool("elevenlabs", "voice", &settings_with_key()).unwrap();
        let b = create_tool("stt", "voice", &settings_with_key()).unwrap();
        assert_eq!(a.name(), "stt.transcribe");
        assert_eq!(b.name(), "stt.transcribe");
    }

    #[test]
    fn require_api_key_errors_when_absent() {
        let s = ToolSettings::default();
        assert!(s.require_api_key("shopify").is_err());
    }
}
