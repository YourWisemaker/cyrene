//! Error model for `cyrene-state`.
//!
//! Follows the design's "Error Handling" convention: one `thiserror` enum per
//! crate, each variant carrying a [`Recoverability`] hint so the Agent_Loop can
//! decide how to react.

use cyrene_core::{Recoverability, Recoverable};

/// Errors raised while opening, checkpointing, or reading the State_Tree.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// An underlying SQLite operation failed.
    #[error("state database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// A stored column held bytes of an unexpected length.
    #[error("stored column `{0}` has an unexpected byte length")]
    CorruptColumn(&'static str),

    /// A checkpoint with the given id was not found.
    /// The second field lists the available checkpoint ids for the session.
    #[error("checkpoint not found: {id}. Available ids: {}", available.join(", "))]
    NotFound {
        /// The id that was requested.
        id: String,
        /// The checkpoint ids that do exist.
        available: Vec<String>,
    },

    /// A blob with the given hash was not found.
    #[error("blob not found for hash")]
    BlobNotFound,

    /// JSON serialization/deserialization of the file manifest failed.
    #[error("manifest serialization error: {0}")]
    ManifestSerde(#[from] serde_json::Error),
}

impl Recoverable for StateError {
    fn recoverability(&self) -> Recoverability {
        match self {
            Self::NotFound { .. } => Recoverability::UserAction,
            Self::Database(_)
            | Self::CorruptColumn(_)
            | Self::BlobNotFound
            | Self::ManifestSerde(_) => Recoverability::Halt,
        }
    }
}
