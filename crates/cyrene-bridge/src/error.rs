//! Error model for the Workspace_Bridge.

/// Errors the Workspace_Bridge can return.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BridgeError {
    /// The target path is outside the configured workspace boundary (R8.5).
    #[error("access denied: `{0}` is outside the workspace boundary")]
    OutOfBounds(String),

    /// A filesystem operation failed.
    #[error("bridge I/O error: {0}")]
    Io(String),
}
