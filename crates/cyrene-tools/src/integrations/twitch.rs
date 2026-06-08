//! Twitch Helix API integration tool.
//!
//! Connects Cyrene to Twitch so the agent can look up streams/channels/users,
//! check whether a channel is live, and (gated) send a chat message or create a
//! clip. The OAuth token is resolved by env-var name into
//! [`ToolSettings::api_key`]; Twitch also requires a Client-Id, supplied via the
//! `client_id` option. Read actions are low risk; chat/clip actions mutate a
//! public surface and are medium risk. The tool validates arguments and models
//! the Helix call; the runtime performs the authenticated HTTP.

use cyrene_core::Risk;

use super::ToolSettings;
use crate::error::ToolError;
use crate::tool::{Tool, ToolOutput};

/// Default Twitch Helix API base URL.
const DEFAULT_BASE_URL: &str = "https://api.twitch.tv/helix";

/// Read actions (low risk).
const READ_ACTIONS: &[&str] = &["get_stream", "get_channel", "get_user", "is_live"];
/// Mutating actions (medium risk — posts to a public surface).
const WRITE_ACTIONS: &[&str] = &["send_chat", "create_clip"];

/// A configured Twitch connection (R28).
pub struct TwitchTool {
    alias: String,
    base_url: String,
    client_id: String,
    #[allow(dead_code)] // Consumed by the runtime that performs the HTTP call.
    oauth_token: String,
}

impl TwitchTool {
    /// Builds the tool from its configured settings.
    ///
    /// # Errors
    /// Returns [`ToolError::InvalidArgs`] if the OAuth token or the `client_id`
    /// option is missing (Helix requires both).
    pub fn new(alias: &str, settings: &ToolSettings) -> Result<Self, ToolError> {
        let oauth_token = settings.require_api_key("twitch")?.to_owned();
        let client_id = settings.require_option("twitch", "client_id")?.to_owned();
        let base_url = settings
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        Ok(Self {
            alias: alias.to_owned(),
            base_url,
            client_id,
            oauth_token,
        })
    }
}

impl Tool for TwitchTool {
    fn name(&self) -> &str {
        "twitch"
    }

    fn default_risk(&self) -> Risk {
        // The tool can post chat messages / create clips on a public surface,
        // so it carries medium risk by default; the gate reviews it.
        Risk::Medium
    }

    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_owned()))?;

        if !READ_ACTIONS.contains(&action) && !WRITE_ACTIONS.contains(&action) {
            return Err(ToolError::InvalidArgs(format!(
                "unknown twitch action `{action}`; supported: {}, {}",
                READ_ACTIONS.join(", "),
                WRITE_ACTIONS.join(", ")
            )));
        }

        match action {
            "get_stream" | "get_channel" | "is_live" | "create_clip" => {
                require_str(args, "broadcaster")?;
            }
            "get_user" => {
                require_str(args, "login")?;
            }
            "send_chat" => {
                require_str(args, "broadcaster")?;
                require_str(args, "message")?;
            }
            _ => {}
        }

        Ok(ToolOutput::ok(format!(
            "[twitch:{}] {action} via {} (client {})",
            self.alias, self.base_url, self.client_id
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

    fn tool() -> TwitchTool {
        let mut options = BTreeMap::new();
        options.insert("client_id".to_owned(), "cid_fake".to_owned());
        TwitchTool::new(
            "main",
            &ToolSettings {
                api_key: Some("oauth_fake".to_owned()),
                options,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn requires_oauth_token() {
        let mut options = BTreeMap::new();
        options.insert("client_id".to_owned(), "cid_fake".to_owned());
        let result = TwitchTool::new(
            "main",
            &ToolSettings {
                options,
                ..Default::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn requires_client_id() {
        let result = TwitchTool::new(
            "main",
            &ToolSettings {
                api_key: Some("oauth_fake".to_owned()),
                ..Default::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn is_live_models_helix_call() {
        let out = tool()
            .run(&json!({ "action": "is_live", "broadcaster": "ninja" }))
            .unwrap();
        assert!(out.success);
        assert!(out.text.contains("is_live"));
        assert!(out.text.contains("api.twitch.tv/helix"));
    }

    #[test]
    fn send_chat_validates_fields() {
        let err = tool()
            .run(&json!({ "action": "send_chat", "broadcaster": "ninja" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn unknown_action_errors() {
        let err = tool()
            .run(&json!({ "action": "ban_everyone" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn twitch_is_medium_risk() {
        assert_eq!(tool().default_risk(), Risk::Medium);
    }

    #[test]
    fn does_not_leak_token() {
        let out = tool()
            .run(&json!({ "action": "is_live", "broadcaster": "x" }))
            .unwrap();
        assert!(!out.text.contains("oauth_fake"));
    }
}
