//! Shopify Admin API integration tool.
//!
//! Connects Cyrene to a Shopify store so the agent can read products/orders and
//! perform store actions. Credentials (the Admin API access token) are resolved
//! by env-var name into [`ToolSettings::api_key`]; the store domain rides in the
//! `shop` option (e.g. `acme.myshopify.com`). The tool validates arguments and
//! models the Admin REST call; the runtime performs the authenticated HTTP I/O.

use cyrene_core::Risk;

use super::ToolSettings;
use crate::error::ToolError;
use crate::tool::{Tool, ToolOutput};

/// Supported Shopify actions and their required arguments.
const ACTIONS: &[&str] = &[
    "list_products",
    "get_product",
    "list_orders",
    "get_order",
    "create_draft_order",
    "adjust_inventory",
];

/// A configured Shopify store connection (R28).
pub struct ShopifyTool {
    alias: String,
    shop: String,
    api_version: String,
    #[allow(dead_code)] // Consumed by the runtime that performs the HTTP call.
    access_token: String,
}

impl ShopifyTool {
    /// Builds the tool from its configured settings.
    ///
    /// # Errors
    /// Returns [`ToolError::InvalidArgs`] if the `shop` option or the access
    /// token is missing.
    pub fn new(alias: &str, settings: &ToolSettings) -> Result<Self, ToolError> {
        let shop = settings.require_option("shopify", "shop")?.to_owned();
        let access_token = settings.require_api_key("shopify")?.to_owned();
        let api_version = settings
            .option("api_version")
            .unwrap_or("2024-10")
            .to_owned();
        Ok(Self {
            alias: alias.to_owned(),
            shop,
            api_version,
            access_token,
        })
    }

    /// The Admin REST base URL for this store.
    #[must_use]
    fn base_url(&self) -> String {
        format!("https://{}/admin/api/{}", self.shop, self.api_version)
    }
}

impl Tool for ShopifyTool {
    fn name(&self) -> &str {
        "shopify"
    }

    fn default_risk(&self) -> Risk {
        // Reads are harmless but the same tool can create draft orders and
        // adjust inventory, so it carries store-mutating risk by default.
        Risk::Medium
    }

    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_owned()))?;
        if !ACTIONS.contains(&action) {
            return Err(ToolError::InvalidArgs(format!(
                "unknown shopify action `{action}`; supported: {}",
                ACTIONS.join(", ")
            )));
        }
        // Per-action argument validation.
        match action {
            "get_product" => require_str(args, "product_id")?,
            "get_order" => require_str(args, "order_id")?,
            "adjust_inventory" => {
                require_str(args, "inventory_item_id")?;
                require_str(args, "location_id")?;
                args.get("available")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| {
                        ToolError::InvalidArgs("missing integer `available`".to_owned())
                    })?;
                ""
            }
            _ => "",
        };
        Ok(ToolOutput::ok(format!(
            "[shopify:{}] {action} via {} (store {})",
            self.alias,
            self.base_url(),
            self.shop
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
    use std::collections::BTreeMap;

    fn tool() -> ShopifyTool {
        let mut options = BTreeMap::new();
        options.insert("shop".to_owned(), "acme.myshopify.com".to_owned());
        ShopifyTool::new(
            "store",
            &ToolSettings {
                api_key: Some("shpat_fake".to_owned()),
                options,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn requires_shop_option() {
        let result = ShopifyTool::new(
            "store",
            &ToolSettings {
                api_key: Some("shpat_fake".to_owned()),
                ..Default::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn requires_access_token() {
        let mut options = BTreeMap::new();
        options.insert("shop".to_owned(), "acme.myshopify.com".to_owned());
        let result = ShopifyTool::new(
            "store",
            &ToolSettings {
                options,
                ..Default::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn list_products_models_admin_call() {
        let out = tool().run(&json!({ "action": "list_products" })).unwrap();
        assert!(out.success);
        assert!(out.text.contains("acme.myshopify.com"));
        assert!(out.text.contains("list_products"));
    }

    #[test]
    fn unknown_action_errors() {
        let err = tool()
            .run(&json!({ "action": "delete_store" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn get_order_requires_order_id() {
        let err = tool().run(&json!({ "action": "get_order" })).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn default_risk_is_medium() {
        assert_eq!(tool().default_risk(), Risk::Medium);
    }

    #[test]
    fn does_not_leak_token_in_output() {
        let out = tool().run(&json!({ "action": "list_products" })).unwrap();
        assert!(!out.text.contains("shpat_fake"));
    }
}
