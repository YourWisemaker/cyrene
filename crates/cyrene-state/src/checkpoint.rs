//! Checkpoint and related types for the State_Tree.
//!
//! A [`Checkpoint`] captures agent memory, file state, and session variables at
//! a specific step, identified by a unique [`CheckpointId`]. File state is
//! stored as a content-addressed manifest mapping relative paths to
//! [`BlobHash`]es (SHA-256 of the file content), enabling copy-on-write
//! deduplication across checkpoints.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use cyrene_core::{BranchId, SessionId};

/// A lightweight summary of a checkpoint, returned by [`StateStore::history`](crate::StateStore::history).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointSummary {
    /// The checkpoint's unique identifier.
    pub id: CheckpointId,
    /// The step sequence number within the session.
    pub step_seq: u64,
    /// A human-readable summary of what happened at this step.
    pub summary: String,
    /// The branch this checkpoint belongs to.
    pub branch_id: BranchId,
}

/// SHA-256 hash of a file blob's content (32 bytes).
pub type BlobHash = [u8; 32];

/// Length of a SHA-256 hash in bytes.
pub const HASH_LEN: usize = 32;

/// Identifies a [`Checkpoint`] in the State_Tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CheckpointId(pub Uuid);

impl CheckpointId {
    /// Generates a fresh, random (v4) identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Wraps an existing [`Uuid`].
    #[must_use]
    pub const fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    /// Returns the underlying [`Uuid`].
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for CheckpointId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<Uuid> for CheckpointId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

/// A recorded snapshot of agent memory, file state, and session variables at a
/// specific step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// Unique identifier for this checkpoint.
    pub id: CheckpointId,
    /// The session this checkpoint belongs to.
    pub session_id: SessionId,
    /// The step sequence number within the session.
    pub step_seq: u64,
    /// The parent checkpoint (if any) — forms the checkpoint chain.
    pub parent_id: Option<CheckpointId>,
    /// The branch this checkpoint belongs to.
    pub branch_id: BranchId,
    /// A human-readable summary of what happened at this step.
    pub summary: String,
    /// Serialized agent memory blob.
    pub mem_blob: Vec<u8>,
    /// Serialized session variables blob.
    pub vars_blob: Vec<u8>,
    /// File manifest: maps relative file paths to their content-addressed
    /// [`BlobHash`]. The actual file bytes are stored in the `blobs` table,
    /// keyed by hash, enabling deduplication across checkpoints.
    pub file_manifest: BTreeMap<String, BlobHash>,
}
