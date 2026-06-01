//! Error model for the Tool_Registry and tools.

/// Errors a tool or the registry can return.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// No tool with the requested name is registered.
    #[error("tool not found: {0}")]
    NotFound(String),

    /// The tool arguments were invalid.
    #[error("invalid tool arguments: {0}")]
    InvalidArgs(String),

    /// The tool failed during execution.
    #[error("tool execution failed: {0}")]
    Execution(String),
}
