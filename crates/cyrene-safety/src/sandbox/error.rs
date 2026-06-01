//! Error model for the Sandbox subsystem.
//!
//! Follows the design's "Error Handling" convention: one `thiserror` enum per
//! subsystem, each variant carrying a [`Recoverability`] hint so the Agent_Loop
//! can decide how to react.

use std::path::PathBuf;

use cyrene_core::{Recoverability, Recoverable};

/// Errors raised while creating, operating, or confining a [`Sandbox`](super::Sandbox).
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// A write (or other mutating operation) was attempted on a path outside
    /// the sandbox boundary. The path is captured for audit/reporting.
    #[error("access denied: path `{path}` is outside the sandbox boundary")]
    DeniedAccess {
        /// The path that was denied.
        path: PathBuf,
        /// Human-readable reason for the denial.
        reason: String,
    },

    /// An underlying I/O operation failed (e.g. creating the temp dir,
    /// copying workspace files).
    #[error("sandbox I/O error: {source}")]
    Io {
        /// What the sandbox was trying to do when the error occurred.
        context: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// OS-level confinement (Landlock, Seatbelt, Job Objects) is not available
    /// on this platform or kernel version. The sandbox still functions via
    /// path-checking, but the OS enforcement layer is absent.
    #[error("OS-level confinement unavailable: {reason}")]
    ConfinementUnavailable {
        /// Why confinement could not be applied.
        reason: String,
    },

    /// The Docker fallback backend failed to start or communicate with the
    /// container runtime.
    #[error("Docker backend error: {reason}")]
    DockerBackendError {
        /// What went wrong with the Docker backend.
        reason: String,
    },
}

impl Recoverable for SandboxError {
    fn recoverability(&self) -> Recoverability {
        match self {
            // Denied access is a policy violation — the user/plan needs to be
            // corrected or the boundary expanded.
            Self::DeniedAccess { .. } => Recoverability::Halt,
            // I/O errors might be transient (disk full, permission race).
            Self::Io { .. } => Recoverability::Retry,
            // Confinement unavailable is informational — the sandbox still
            // works via path-checking, but the user should be aware.
            Self::ConfinementUnavailable { .. } => Recoverability::UserAction,
            // Docker backend failure might be transient (daemon not running).
            Self::DockerBackendError { .. } => Recoverability::Retry,
        }
    }
}
