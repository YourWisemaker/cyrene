//! YouTube Data API integration tool.
//!
//! Connects Cyrene to YouTube so the agent can search videos, read video and
//! channel metadata, list comments, and (gated) post a comment. The API key /
//! OAuth token is resolved by env-var name into [`ToolSettings::api_key`]. Read
//! actions are low risk; posting a comment mutates a public surface and is
//! medium risk, so the autonomy gate reviews it. The tool validates arguments
//! and models the Data API call; the runtime performs the authenticated HTTP.

use cyrene_core::Risk;

use super::ToolSettings;
use crate::error::ToolError;
use crate::tool::{Tool, ToolOutput};

/// Default YouTube Data API v3 base URL.
const DEFAULT_BASE_URL: &str = "https://www.googleapis.com/youtube/v3";

/// Read actions (low risk).
const READ_ACTIONS: &[&str] = &["search", "get_video", "get_channel", "list_comments"];
/// Mutating actions (medium risk — posts to a public surface).
const WRITE_ACTIONS: &[&str] = &["post_comment"];

/// A configured YouTube connection (R28).
pub struct YouTubeTool {
    alias: String,
    base_url: String,
    #[allow(dead_code)] // Consumed by the runtime that performs the HTTP call.
    api_key: String,
}

impl YouTubeTool {
    /// Builds the tool from its configured settings.
    ///
    /// # Errors
    /// Returns [`ToolError::InvalidArgs`] if the API key/OAuth token is missing.
    pub fn new(alias: &str, settings: &ToolSettings) -> Result<Self, ToolError> {
        let api_key = settings.require_api_key("youtube")?.to_owned();
        let base_url = settings
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        Ok(Self {
            alias: alias.to_owned(),
            base_url,
            api_key,
        })
    }
}

impl Tool for YouTubeTool {
    fn name(&self) -> &str {
        "youtube"
    }

    fn default_risk(&self) -> Risk {
        // The tool can post comments to a public surface, so it carries
        // medium risk by default; the gate reviews it regardless of action.
        Risk::Medium
    }

    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_owned()))?;

        if !READ_ACTIONS.contains(&action) && !WRITE_ACTIONS.contains(&action) {
            return Err(ToolError::InvalidArgs(format!(
                "unknown youtube action `{action}`; supported: {}, {}",
                READ_ACTIONS.join(", "),
                WRITE_ACTIONS.join(", ")
            )));
        }

        match action {
            "search" => {
                require_str(args, "query")?;
            }
            "get_video" | "list_comments" => {
                require_str(args, "video_id")?;
            }
            "get_channel" => {
                require_str(args, "channel_id")?;
            }
            "post_comment" => {
                require_str(args, "video_id")?;
                require_str(args, "text")?;
            }
            _ => {}
        }

        Ok(ToolOutput::ok(format!(
            "[youtube:{}] {action} via {}",
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

    fn tool() -> YouTubeTool {
        YouTubeTool::new(
            "main",
            &ToolSettings {
                api_key: Some("yt_fake".to_owned()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn requires_api_key() {
        assert!(YouTubeTool::new("main", &ToolSettings::default()).is_err());
    }

    #[test]
    fn search_models_data_api_call() {
        let out = tool()
            .run(&json!({ "action": "search", "query": "rust async" }))
            .unwrap();
        assert!(out.success);
        assert!(out.text.contains("search"));
        assert!(out.text.contains("googleapis.com/youtube/v3"));
    }

    #[test]
    fn get_video_requires_video_id() {
        let err = tool().run(&json!({ "action": "get_video" })).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn post_comment_validates_fields() {
        let err = tool()
            .run(&json!({ "action": "post_comment", "video_id": "abc" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn unknown_action_errors() {
        let err = tool()
            .run(&json!({ "action": "delete_video" }))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn youtube_is_medium_risk() {
        assert_eq!(tool().default_risk(), Risk::Medium);
    }

    #[test]
    fn does_not_leak_key() {
        let out = tool()
            .run(&json!({ "action": "search", "query": "x" }))
            .unwrap();
        assert!(!out.text.contains("yt_fake"));
    }
}
