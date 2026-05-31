//! Error model for `cyrene-ledger`.
//!
//! Follows the design's "Error Handling" convention: one `thiserror` enum per
//! crate, each variant carrying a [`Recoverability`] hint so the Agent_Loop can
//! decide how to react. Ledger failures are mostly unrecoverable
//! (`Halt` — a corrupt chain or DB error cannot be auto-recovered) or need the
//! user to act (`UserAction` — a missing/malformed install key).

use std::path::PathBuf;

use cyrene_core::{Recoverability, Recoverable};

/// Errors raised while opening, appending to, or reading the Receipt_Ledger.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// An underlying SQLite operation failed.
    #[error("ledger database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// The install key file could not be read or written.
    #[error("failed to access install key file `{path}`: {source}")]
    KeyIo {
        /// The key-file path involved in the failure.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// The install key file did not contain a well-formed ed25519 secret seed.
    #[error("install key file `{0}` is malformed (expected exactly 32 bytes)")]
    KeyFormat(PathBuf),

    /// A stored receipt column held bytes of an unexpected length (a 32-byte
    /// hash or 64-byte signature), indicating a corrupted row.
    #[error("stored receipt column `{0}` has an unexpected byte length")]
    CorruptColumn(&'static str),

    /// The receipt sequence number overflowed the representable range.
    #[error("receipt sequence number overflowed the supported range")]
    SeqOverflow,

    /// A stored timestamp could not be parsed back into a UTC datetime.
    #[error("failed to parse stored receipt timestamp: {0}")]
    Timestamp(String),
}

impl Recoverable for LedgerError {
    fn recoverability(&self) -> Recoverability {
        match self {
            // A missing or malformed install key needs the user to fix it
            // (regenerate/restore the keypair).
            Self::KeyIo { .. } | Self::KeyFormat(_) => Recoverability::UserAction,
            // Everything else is an integrity/IO failure we cannot auto-recover.
            Self::Database(_) | Self::CorruptColumn(_) | Self::SeqOverflow | Self::Timestamp(_) => {
                Recoverability::Halt
            }
        }
    }
}
