//! Built-in tools (R28.1).
//!
//! Cyrene ships a baseline tool suite: file operations, terminal/code
//! execution, web fetch + search, image generation, and text-to-speech. Each
//! declares a default [`Risk`] so the autonomy policy and Approval_Gate apply
//! (R28.4):
//!
//! - File read / web fetch / web search → low (read-only).
//! - File write → medium (mutates the workspace).
//! - Terminal / code execution → medium (sandboxed).
//! - Image generation / text-to-speech → low (produces assets).
//!
//! These implementations validate arguments and model the action; the actual
//! side-effecting I/O is wired by the runtime (sandbox, HTTP client, TTS
//! engine) behind the same trait. The default registry wires the safe,
//! self-contained ones so the suite is testable end to end.

use cyrene_core::Risk;

use crate::error::ToolError;
use crate::tool::{Tool, ToolOutput, ToolRegistry};

/// Reads a file's contents within the workspace (low risk).
pub struct FileReadTool;
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "fs.read"
    }
    fn default_risk(&self) -> Risk {
        Risk::Low
    }
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let path = require_str(args, "path")?;
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(ToolOutput::ok(content)),
            Err(e) => Err(ToolError::Execution(format!("read {path}: {e}"))),
        }
    }
}

/// Writes content to a file (medium risk — mutates the workspace).
pub struct FileWriteTool;
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "fs.write"
    }
    fn default_risk(&self) -> Risk {
        Risk::Medium
    }
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let path = require_str(args, "path")?;
        let content = require_str(args, "content")?;
        match std::fs::write(path, content) {
            Ok(()) => Ok(ToolOutput::ok(format!(
                "wrote {} bytes to {path}",
                content.len()
            ))),
            Err(e) => Err(ToolError::Execution(format!("write {path}: {e}"))),
        }
    }
}

/// Runs a terminal command (medium risk — gated/sandboxed by the runtime).
///
/// This built-in does not execute anything itself; it records the intended
/// command. The runtime's sandbox + autonomy gate decides whether and how it
/// actually runs, so the tool stays safe-by-default in tests.
pub struct TerminalTool;
impl Tool for TerminalTool {
    fn name(&self) -> &str {
        "shell.run"
    }
    fn default_risk(&self) -> Risk {
        Risk::Medium
    }
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let cmd = require_str(args, "cmd")?;
        Ok(ToolOutput::ok(format!("[sandboxed] would run: {cmd}")))
    }
}

/// Fetches a URL (low risk — read-only). Records the request; the runtime
/// supplies the HTTP client.
pub struct WebFetchTool;
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web.fetch"
    }
    fn default_risk(&self) -> Risk {
        Risk::Low
    }
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let url = require_str(args, "url")?;
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolError::InvalidArgs(format!("not an http(s) url: {url}")));
        }
        Ok(ToolOutput::ok(format!("[fetch] {url}")))
    }
}

/// Searches the web (low risk). Records the query; the runtime supplies the
/// search backend.
pub struct WebSearchTool;
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web.search"
    }
    fn default_risk(&self) -> Risk {
        Risk::Low
    }
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let query = require_str(args, "query")?;
        Ok(ToolOutput::ok(format!("[search] {query}")))
    }
}

/// Generates an image from a prompt (low risk — produces an asset).
pub struct ImageGenTool;
impl Tool for ImageGenTool {
    fn name(&self) -> &str {
        "image.generate"
    }
    fn default_risk(&self) -> Risk {
        Risk::Low
    }
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let prompt = require_str(args, "prompt")?;
        Ok(ToolOutput::ok(format!("[image] generated for: {prompt}")))
    }
}

/// Converts text to speech (low risk — produces an asset).
pub struct TextToSpeechTool;
impl Tool for TextToSpeechTool {
    fn name(&self) -> &str {
        "tts.speak"
    }
    fn default_risk(&self) -> Risk {
        Risk::Low
    }
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        let text = require_str(args, "text")?;
        Ok(ToolOutput::ok(format!(
            "[tts] {} chars synthesized",
            text.len()
        )))
    }
}

/// Builds a registry pre-populated with the built-in tool suite (R28.1).
#[must_use]
pub fn builtin_registry() -> ToolRegistry {
    let mut r = ToolRegistry::new();
    r.register(Box::new(FileReadTool));
    r.register(Box::new(FileWriteTool));
    r.register(Box::new(TerminalTool));
    r.register(Box::new(WebFetchTool));
    r.register(Box::new(WebSearchTool));
    r.register(Box::new(ImageGenTool));
    r.register(Box::new(TextToSpeechTool));
    r
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
    use cyrene_core::ToolCall;
    use serde_json::json;

    #[test]
    fn builtin_registry_has_full_suite() {
        let r = builtin_registry();
        for name in [
            "fs.read",
            "fs.write",
            "shell.run",
            "web.fetch",
            "web.search",
            "image.generate",
            "tts.speak",
        ] {
            assert!(r.contains(name), "missing built-in tool {name}");
        }
        assert_eq!(r.len(), 7);
    }

    #[test]
    fn risk_levels_are_assigned() {
        let r = builtin_registry();
        assert_eq!(r.risk_of("fs.read"), Some(Risk::Low));
        assert_eq!(r.risk_of("fs.write"), Some(Risk::Medium));
        assert_eq!(r.risk_of("shell.run"), Some(Risk::Medium));
        assert_eq!(r.risk_of("web.fetch"), Some(Risk::Low));
    }

    #[test]
    fn file_write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        let path_str = path.to_str().unwrap();
        let r = builtin_registry();

        let (out, _) = r
            .dispatch(&ToolCall::new(
                "fs.write",
                json!({ "path": path_str, "content": "hello tools" }),
            ))
            .unwrap();
        assert!(out.success);

        let (read, _) = r
            .dispatch(&ToolCall::new("fs.read", json!({ "path": path_str })))
            .unwrap();
        assert_eq!(read.text, "hello tools");
    }

    #[test]
    fn web_fetch_rejects_non_http_url() {
        let r = builtin_registry();
        let err = r
            .dispatch(&ToolCall::new("web.fetch", json!({ "url": "ftp://x" })))
            .unwrap_err();
        assert!(matches!(
            err,
            ToolError::Execution(_) | ToolError::InvalidArgs(_)
        ));
    }

    #[test]
    fn missing_required_arg_errors() {
        let r = builtin_registry();
        let err = r
            .dispatch(&ToolCall::new("web.search", json!({})))
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn terminal_tool_does_not_execute_in_test() {
        let r = builtin_registry();
        let (out, _) = r
            .dispatch(&ToolCall::new("shell.run", json!({ "cmd": "rm -rf /" })))
            .unwrap();
        // Safe-by-default: the built-in only records the intent.
        assert!(out.text.contains("[sandboxed] would run"));
    }
}
