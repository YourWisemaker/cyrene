//! Generic marketplace integration tool.
//!
//! Connects Cyrene to an e-commerce / listings marketplace over an HTTP API so
//! the agent can search listings, fetch an item, and (gated) create a listing
//! or place an order. The marketplace is identified by its `base_url` option;
//! the credential is resolved by env-var name into [`ToolSettings::api_key`].
//! This is intentionally provider-agnostic — point it at a specific
//! marketplace's REST API via `base_url` and select behavior per call.

use cyrene_core::Risk;

use super::ToolSettings;
use crate::error::ToolError;
use crate::tool::{Tool, ToolOutput};

/// Read actions (low risk).
const READ_ACTIONS: &[&str] = &["search", "get_listing", "list_orders"];
/// Mutating actions (medium risk — money/inventory changes).
const WRITE_ACTIONS: &[&str] = &["create_listing", "update_listing", "place_order"];

/// A configured marketplace connection (R28).
pub struct MarketplaceTool {
    alias: String,
    base_url: String,
    #[allow(dead_code)] // Consumed by the runtime that performs the HTTP call.
    api_key: Option<String>,
}

impl MarketplaceTool {
    /// Builds the marketplace connection from its configured settings.
    ///
    /// # Errors
    /// Returns [`ToolError::InvalidArgs`] if neither `base_url` (setting) nor a
    /// `base_url` option is provided.
    pub fn new(alias: &str, settings: &ToolSettings) -> Result<Self, ToolError> {
        let base_url = settings
            .base_url
            .clone()
            .or_else(|| settings.option("base_url").map(str::to_owned))
            .ok_or_else(|| {
                ToolError::InvalidArgs("marketplace requires a `base_url`".to_owned())
            })?;
        Ok(Self {
            alias: alias.to_owned(),
            base_url,
            api_key: settings.api_key.clone(),
        })
    }
}

impl Tool for MarketplaceTool {
    fn name(&self) -> &str {
        "marketplace"
    }

    fn default_risk(&self) -> Risk {
        // The tool can create listings and place orders (money changes hands),
        // so it carries store-mutating risk by default.
        Risk::Medium
    }

    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_owned()))?;

        if !READ_ACTIONS.contains(&action) && !WRITE_ACTIONS.contains(&action) {
            return Err(ToolError::InvalidArgs(format!(
                "unknown marketplace action `{action}`; supported: {}, {}",
                READ_ACTIONS.join(", "),
                WRITE_ACTIONS.join(", ")
            )));
        }

        match action {
            "search" => {
                require_str(args, "query")?;
            }
            "get_listing" => {
                require_str(args, "listing_id")?;
            }
            "place_order" => {
                require_str(args, "listing_id")?;
            }
            "create_listing" => {
                require_str(args, "title")?;
                require_str(args, "price")?;
            }
            _ => {}
        }

        // Mutating actions require an authenticated credential.
        if WRITE_ACTIONS.contains(&action) && self.api_key.is_none() {
            return Err(ToolError::InvalidArgs(format!(
                "marketplace action `{action}` requires an API key (set api_key_env)"
            )));
        }

        Ok(ToolOutput::ok(format!(
            "[marketplace:{}] {action} via {}",
            self.alias, self.base_url
        )))
    }
}

/// Extracts a required string argument, erroring if absent.
fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidArgs(format!("missing `{key}`")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn market(with_key: bool) -> MarketplaceTool {
        MarketplaceTool::new(
            "shop",
            &ToolSettings {
                api_key: with_key.then(|| "mk_fake".to_owned()),
                base_url: Some("https://market.example/api".to_owned()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn requires_base_url() {
        assert!(MarketplaceTool::new("shop", &ToolSettings::default()).is_err());
    }

    #[test]
    fn search_works_without_key() {
        let out = market(false)
            .run(&json!({ "action": "search", "query": "vintage chair" }))
            .unwrap();
        assert!(out.text.contains("search"));
        assert!(out.text.contains("market.example"));
    }

    #[test]
    fn place_order_requires_key() {
        let err = market(false)
            .run(&json!({ "action": "place_order", "listing_id": "42" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn create_listing_validates_fields() {
        let err = market(true)
            .run(&json!({ "action": "create_listing", "title": "Chair" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn marketplace_is_medium_risk() {
        assert_eq!(market(true).default_risk(), Risk::Medium);
    }

    #[test]
    fn unknown_action_errors() {
        let err = market(true).run(&json!({ "action": "nuke" })).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }
}
