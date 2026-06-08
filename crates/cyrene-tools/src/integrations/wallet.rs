//! Crypto wallet integration tool.
//!
//! Lets the agent query balances and (gated) submit transactions against an
//! EVM-compatible chain via a JSON-RPC endpoint. The signing key is resolved by
//! env-var name into [`ToolSettings::api_key`] and is **never** logged; the RPC
//! URL and network ride in the options. Read actions (balance, gas) are low
//! risk; value-moving actions are High risk so the autonomy policy and
//! Approval_Gate always gate them (R28.4) — sending funds is irreversible.

use cyrene_core::Risk;

use super::ToolSettings;
use crate::error::ToolError;
use crate::tool::{Tool, ToolOutput};

/// Read-only wallet actions (low risk).
const READ_ACTIONS: &[&str] = &["balance", "gas_price", "tx_status"];
/// Value-moving wallet actions (high risk, irreversible).
const WRITE_ACTIONS: &[&str] = &["send", "approve", "swap"];

/// A configured crypto wallet connection (R28).
pub struct WalletTool {
    alias: String,
    network: String,
    rpc_url: String,
    #[allow(dead_code)] // Consumed by the runtime that signs/sends transactions.
    signing_key: Option<String>,
}

impl WalletTool {
    /// Builds the wallet from its configured settings.
    ///
    /// # Errors
    /// Returns [`ToolError::InvalidArgs`] if the `rpc_url` option is missing.
    /// A signing key is optional: a read-only wallet (balances/quotes) needs
    /// none, while value-moving actions require it at run time.
    pub fn new(alias: &str, settings: &ToolSettings) -> Result<Self, ToolError> {
        let rpc_url = settings.require_option("wallet", "rpc_url")?.to_owned();
        let network = settings.option("network").unwrap_or("ethereum").to_owned();
        Ok(Self {
            alias: alias.to_owned(),
            network,
            rpc_url,
            signing_key: settings.api_key.clone(),
        })
    }
}

impl Tool for WalletTool {
    fn name(&self) -> &str {
        "wallet"
    }

    fn default_risk(&self) -> Risk {
        // The tool can move funds irreversibly, so it carries high risk; the
        // gate always reviews it regardless of the specific action.
        Risk::High
    }

    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_owned()))?;

        let is_read = READ_ACTIONS.contains(&action);
        let is_write = WRITE_ACTIONS.contains(&action);
        if !is_read && !is_write {
            return Err(ToolError::InvalidArgs(format!(
                "unknown wallet action `{action}`; supported: {}, {}",
                READ_ACTIONS.join(", "),
                WRITE_ACTIONS.join(", ")
            )));
        }

        match action {
            "balance" => {
                require_str(args, "address")?;
            }
            "tx_status" => {
                require_str(args, "tx_hash")?;
            }
            "send" => {
                require_str(args, "to")?;
                require_str(args, "amount")?;
            }
            "approve" | "swap" => {
                require_str(args, "token")?;
            }
            _ => {}
        }

        // Value-moving actions require a signing key to exist.
        if is_write && self.signing_key.is_none() {
            return Err(ToolError::InvalidArgs(format!(
                "wallet action `{action}` requires a signing key (set api_key_env)"
            )));
        }

        Ok(ToolOutput::ok(format!(
            "[wallet:{}] {action} on {} via RPC {}",
            self.alias, self.network, self.rpc_url
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

    fn wallet(with_key: bool) -> WalletTool {
        let mut options = BTreeMap::new();
        options.insert("rpc_url".to_owned(), "https://eth.example/rpc".to_owned());
        WalletTool::new(
            "main",
            &ToolSettings {
                api_key: with_key.then(|| "0xprivkey".to_owned()),
                options,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn requires_rpc_url() {
        assert!(WalletTool::new("main", &ToolSettings::default()).is_err());
    }

    #[test]
    fn read_only_wallet_builds_without_key() {
        let out = wallet(false)
            .run(&json!({ "action": "balance", "address": "0xabc" }))
            .unwrap();
        assert!(out.text.contains("balance"));
        assert!(out.text.contains("ethereum"));
    }

    #[test]
    fn send_requires_signing_key() {
        let err = wallet(false)
            .run(&json!({ "action": "send", "to": "0xdef", "amount": "1.0" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn send_with_key_models_tx() {
        let out = wallet(true)
            .run(&json!({ "action": "send", "to": "0xdef", "amount": "1.0" }))
            .unwrap();
        assert!(out.text.contains("send"));
    }

    #[test]
    fn wallet_is_high_risk() {
        assert_eq!(wallet(true).default_risk(), Risk::High);
    }

    #[test]
    fn unknown_action_errors() {
        let err = wallet(true).run(&json!({ "action": "rug" })).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn does_not_leak_signing_key() {
        let out = wallet(true)
            .run(&json!({ "action": "send", "to": "0xdef", "amount": "1.0" }))
            .unwrap();
        assert!(!out.text.contains("0xprivkey"));
    }
}
