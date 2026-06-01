//! `cyrene-state`: the git-style State_Tree and checkpoint/rollback store for Cyrene.
//!
//! This crate implements the State_Tree (R4): a SQLite-backed, content-addressed
//! checkpoint system that records agent memory, file state, and session variables
//! at each step. File content is stored as SHA-256-hashed blobs with
//! copy-on-write deduplication — identical file content across checkpoints
//! occupies only one blob row.
//!
//! - [`StateStore::checkpoint`]: records a new checkpoint with content-addressed
//!   file blobs (SHA-256 deduplication).
//! - [`StateStore::get`]: retrieves a checkpoint by id (metadata + manifest).
//! - [`StateStore::read_blob`]: retrieves raw file bytes by hash.
//! - [`StateStore::checkout`]: restores a checkpoint by id, returning an error
//!   listing valid ids if the id is unknown (R4.2, R4.3).
//! - [`StateStore::history`]: returns ordered checkpoint summaries for a session
//!   (R4.4).
//! - [`StateStore::fork_branch`]: allocates a new branch when advancing from a
//!   restored checkpoint, preserving superseded checkpoints (R4.5).

mod checkpoint;
mod error;
mod store;

pub use checkpoint::{BlobHash, Checkpoint, CheckpointId, CheckpointSummary, HASH_LEN};
pub use error::StateError;
pub use store::{sha256, StateStore};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-state"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
