//! `cyrene-tools`: the Tool_Registry, built-in tools, and MCP client (R28).
//!
//! - [`Tool`] + [`ToolRegistry`] — register tools by name and dispatch a
//!   [`cyrene_core::ToolCall`] to the matching tool (R28.2). Each tool declares
//!   a default [`cyrene_core::Risk`] so the autonomy policy and Approval_Gate
//!   apply before a medium+/irreversible tool runs (R28.4).
//! - [`builtin_registry`] — the baseline suite: file ops, terminal/code
//!   execution, web fetch + search, image generation, text-to-speech (R28.1).
//! - [`register_mcp_servers`] — registers tools from configured MCP servers
//!   alongside the built-ins, skipping + recording unavailable servers (R28.5,
//!   R28.6).
//! - [`compress_tool_output`] — compresses a tool's output before it enters
//!   model context (R28.3), reusing `cyrene-compress`.

mod builtins;
mod error;
mod mcp;
mod tool;

pub use builtins::{
    builtin_registry, FileReadTool, FileWriteTool, ImageGenTool, TerminalTool, TextToSpeechTool,
    WebFetchTool, WebSearchTool,
};
pub use error::ToolError;
pub use mcp::{register_mcp_servers, McpConnection, McpLoadFailure, McpToolDescriptor};
pub use tool::{Tool, ToolInvocation, ToolOutput, ToolRegistry};

use cyrene_compress::{CompressedOutput, OutputCompressor};

/// Compresses a tool's output before it enters model context (R28.3).
///
/// Thin wrapper over `cyrene-compress` so callers wire tool dispatch and
/// compression together.
#[must_use]
pub fn compress_tool_output(
    compressor: &OutputCompressor,
    output: &ToolOutput,
    tool_name: &str,
) -> CompressedOutput {
    compressor.compress(&output.text, tool_name)
}

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-tools"
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrene_compress::{CompressConfig, OutputCompressor};

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }

    #[test]
    fn tool_output_is_compressed_for_model_context() {
        // A large tool output is compressed before entering model context (R28.3).
        let compressor = OutputCompressor::new(CompressConfig {
            max_chars: 200,
            ..Default::default()
        });
        let long = (0..500)
            .map(|i| format!("output line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let output = ToolOutput::ok(long);
        let compressed = compress_tool_output(&compressor, &output, "shell.run");
        assert!(compressed.compressed_chars < compressed.original_chars);
    }
}
