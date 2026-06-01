//! The SQLite-backed State_Tree store.
//!
//! [`StateStore`] owns a `rusqlite` connection (embedded SQLite via the
//! `bundled` feature, so no system SQLite is required). Opening a store
//! initializes the schema if absent. The store provides:
//!
//! - [`StateStore::checkpoint`]: records a new checkpoint with content-addressed
//!   file blobs (SHA-256 deduplication).
//! - [`StateStore::get`]: retrieves a checkpoint by id (metadata + manifest).
//! - [`StateStore::read_blob`]: retrieves raw file bytes by hash.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use cyrene_core::{BranchId, SessionId};

use crate::checkpoint::{BlobHash, Checkpoint, CheckpointId, HASH_LEN};
use crate::error::StateError;

/// The schema for the state tree tables.
///
/// `checkpoints` stores checkpoint metadata and the file manifest as JSON.
/// `blobs` is a content-addressed store keyed by SHA-256 hash.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS checkpoints (
    id            TEXT    PRIMARY KEY,
    session_id    TEXT    NOT NULL,
    step_seq      INTEGER NOT NULL,
    parent_id     TEXT,
    branch_id     TEXT    NOT NULL,
    summary       TEXT    NOT NULL,
    mem_blob      BLOB    NOT NULL,
    vars_blob     BLOB    NOT NULL,
    file_manifest TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_checkpoints_session ON checkpoints (session_id, step_seq);

CREATE TABLE IF NOT EXISTS blobs (
    hash    BLOB PRIMARY KEY CHECK (length(hash) = 32),
    content BLOB NOT NULL
);";

/// The git-style State_Tree store backed by SQLite.
#[derive(Debug)]
pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    /// Opens (or creates) a state store at `db_path`.
    ///
    /// The schema is initialized on open if absent. Pass an explicit path so
    /// tests can target a temp file; production wiring uses the application
    /// data directory.
    ///
    /// # Errors
    /// Returns [`StateError`] if the database cannot be opened/initialized.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, StateError> {
        let conn = Connection::open(db_path)?;
        Self::from_connection(conn)
    }

    /// Opens an in-memory state store. Intended for tests.
    ///
    /// # Errors
    /// Returns [`StateError`] if the schema cannot be initialized.
    pub fn open_in_memory() -> Result<Self, StateError> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn)
    }

    /// Initializes the schema and wraps the connection into a [`StateStore`].
    fn from_connection(conn: Connection) -> Result<Self, StateError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Records a new checkpoint, storing file blobs deduplicated by SHA-256
    /// hash, and returns the generated [`CheckpointId`].
    ///
    /// `files` is a slice of `(relative_path, content_bytes)` pairs. Each
    /// file's content is hashed; if the blob already exists in the store it is
    /// not re-inserted (copy-on-write deduplication). The resulting manifest
    /// maps each path to its content hash.
    ///
    /// # Errors
    /// Returns [`StateError`] on a database failure or manifest serialization
    /// error.
    #[allow(clippy::too_many_arguments)]
    pub fn checkpoint(
        &self,
        session_id: SessionId,
        step_seq: u64,
        parent_id: Option<CheckpointId>,
        branch_id: BranchId,
        summary: impl Into<String>,
        mem_blob: &[u8],
        vars_blob: &[u8],
        files: &[(&str, &[u8])],
    ) -> Result<CheckpointId, StateError> {
        let id = CheckpointId::new();
        let summary = summary.into();

        // Build the file manifest and store blobs (deduped by hash).
        let mut file_manifest: BTreeMap<String, BlobHash> = BTreeMap::new();
        for &(path, content) in files {
            let hash = sha256(content);
            self.store_blob_if_absent(&hash, content)?;
            file_manifest.insert(path.to_owned(), hash);
        }

        let manifest_json = serde_json::to_string(&file_manifest)?;

        self.conn.execute(
            "INSERT INTO checkpoints
                (id, session_id, step_seq, parent_id, branch_id, summary,
                 mem_blob, vars_blob, file_manifest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                id.to_string(),
                session_id.to_string(),
                step_seq as i64,
                parent_id.map(|p| p.to_string()),
                branch_id.to_string(),
                summary,
                mem_blob,
                vars_blob,
                manifest_json,
            ],
        )?;

        Ok(id)
    }

    /// Retrieves a checkpoint by id, including its metadata and file manifest.
    ///
    /// Returns `None` if no checkpoint with the given id exists.
    ///
    /// # Errors
    /// Returns [`StateError`] on a database failure or corrupt stored data.
    pub fn get(&self, id: CheckpointId) -> Result<Option<Checkpoint>, StateError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, session_id, step_seq, parent_id, branch_id, summary,
                        mem_blob, vars_blob, file_manifest
                 FROM checkpoints
                 WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Vec<u8>>(6)?,
                        row.get::<_, Vec<u8>>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                },
            )
            .optional()?;

        match row {
            Some((
                _id_str,
                session_str,
                step_seq,
                parent_str,
                branch_str,
                summary,
                mem_blob,
                vars_blob,
                manifest_json,
            )) => {
                let session_id = parse_session_id(&session_str)?;
                let parent_id = parent_str.map(|s| parse_checkpoint_id(&s)).transpose()?;
                let branch_id = parse_branch_id(&branch_str)?;
                let file_manifest: BTreeMap<String, BlobHash> =
                    serde_json::from_str(&manifest_json)?;

                Ok(Some(Checkpoint {
                    id,
                    session_id,
                    step_seq: step_seq as u64,
                    parent_id,
                    branch_id,
                    summary,
                    mem_blob,
                    vars_blob,
                    file_manifest,
                }))
            }
            None => Ok(None),
        }
    }

    /// Reads the raw content of a blob by its SHA-256 hash.
    ///
    /// Returns `None` if no blob with the given hash exists.
    ///
    /// # Errors
    /// Returns [`StateError`] on a database failure.
    pub fn read_blob(&self, hash: &BlobHash) -> Result<Option<Vec<u8>>, StateError> {
        let content = self
            .conn
            .query_row(
                "SELECT content FROM blobs WHERE hash = ?1",
                [hash.as_slice()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        Ok(content)
    }

    /// Returns the number of distinct blobs in the content-addressed store.
    ///
    /// # Errors
    /// Returns [`StateError`] on a database failure.
    pub fn blob_count(&self) -> Result<u64, StateError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM blobs", [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    /// Retrieves a checkpoint by id for restoration (R4.2).
    ///
    /// Returns the full [`Checkpoint`] including `mem_blob`, `vars_blob`, and
    /// `file_manifest` so the caller can restore agent state. File blob content
    /// is retrievable via [`StateStore::read_blob`].
    ///
    /// If the id does not exist, returns a [`StateError::NotFound`] that lists
    /// the available checkpoint ids (R4.3).
    ///
    /// # Errors
    /// Returns [`StateError::NotFound`] if the id is unknown, or
    /// [`StateError::Database`] / [`StateError::ManifestSerde`] on internal
    /// failures.
    pub fn checkout(&self, id: CheckpointId) -> Result<Checkpoint, StateError> {
        match self.get(id)? {
            Some(cp) => Ok(cp),
            None => {
                let available = self.all_checkpoint_ids()?;
                Err(StateError::NotFound {
                    id: id.to_string(),
                    available,
                })
            }
        }
    }

    /// Returns the ordered list of checkpoints for a session (R4.4).
    ///
    /// Checkpoints are ordered by `step_seq` ascending. Returns an empty
    /// `Vec` if the session has no checkpoints.
    ///
    /// # Errors
    /// Returns [`StateError`] on a database failure.
    pub fn history(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<crate::checkpoint::CheckpointSummary>, StateError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, step_seq, summary, branch_id
             FROM checkpoints
             WHERE session_id = ?1
             ORDER BY step_seq ASC",
        )?;

        let rows = stmt.query_map([session_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            let (id_str, step_seq, summary, branch_str) = row?;
            let id = parse_checkpoint_id(&id_str)?;
            let branch_id = parse_branch_id(&branch_str)?;
            summaries.push(crate::checkpoint::CheckpointSummary {
                id,
                step_seq: step_seq as u64,
                summary,
                branch_id,
            });
        }

        Ok(summaries)
    }

    /// Returns the checkpoint ids available for a given session (R4.3 helper).
    ///
    /// Used by [`StateStore::checkout`] to populate the error message when a
    /// requested id does not exist.
    ///
    /// # Errors
    /// Returns [`StateError`] on a database failure.
    pub fn available_checkpoint_ids(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<CheckpointId>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM checkpoints WHERE session_id = ?1 ORDER BY step_seq ASC")?;

        let rows = stmt.query_map([session_id.to_string()], |row| row.get::<_, String>(0))?;

        let mut ids = Vec::new();
        for row in rows {
            let id_str = row?;
            ids.push(parse_checkpoint_id(&id_str)?);
        }
        Ok(ids)
    }

    /// Allocates a new branch for advancing from a restored checkpoint (R4.5).
    ///
    /// When the Agent_Loop proceeds from a restored checkpoint, it calls this
    /// method to obtain a fresh [`BranchId`]. Subsequent checkpoints are
    /// recorded under the new branch, preserving the superseded checkpoints on
    /// their original branch. This ensures the append-only invariant: advancing
    /// from a restored checkpoint never deletes or overwrites superseded
    /// checkpoints.
    ///
    /// # Errors
    /// Returns [`StateError::NotFound`] if `from_checkpoint_id` does not exist.
    pub fn fork_branch(&self, from_checkpoint_id: CheckpointId) -> Result<BranchId, StateError> {
        // Verify the source checkpoint exists.
        if self.get(from_checkpoint_id)?.is_none() {
            let available = self.all_checkpoint_ids()?;
            return Err(StateError::NotFound {
                id: from_checkpoint_id.to_string(),
                available,
            });
        }
        // Generate a fresh branch id. The caller uses this for subsequent
        // checkpoints, leaving the original branch's checkpoints intact.
        Ok(BranchId::new())
    }

    /// Returns all checkpoint ids across all sessions (used for error messages).
    fn all_checkpoint_ids(&self) -> Result<Vec<String>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM checkpoints ORDER BY step_seq ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    /// Stores a blob if it does not already exist (content-addressed dedup).
    fn store_blob_if_absent(&self, hash: &BlobHash, content: &[u8]) -> Result<(), StateError> {
        // INSERT OR IGNORE: if the hash already exists, this is a no-op.
        self.conn.execute(
            "INSERT OR IGNORE INTO blobs (hash, content) VALUES (?1, ?2)",
            rusqlite::params![hash.as_slice(), content],
        )?;
        Ok(())
    }
}

/// Computes the SHA-256 hash of `data`.
pub fn sha256(data: &[u8]) -> BlobHash {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hash = [0u8; HASH_LEN];
    hash.copy_from_slice(&result);
    hash
}

/// Parses a stored UUID string into a [`SessionId`].
fn parse_session_id(s: &str) -> Result<SessionId, StateError> {
    s.parse::<uuid::Uuid>()
        .map(SessionId::from_uuid)
        .map_err(|_| StateError::CorruptColumn("session_id"))
}

/// Parses a stored UUID string into a [`CheckpointId`].
fn parse_checkpoint_id(s: &str) -> Result<CheckpointId, StateError> {
    s.parse::<uuid::Uuid>()
        .map(CheckpointId::from_uuid)
        .map_err(|_| StateError::CorruptColumn("parent_id"))
}

/// Parses a stored UUID string into a [`BranchId`].
fn parse_branch_id(s: &str) -> Result<BranchId, StateError> {
    s.parse::<uuid::Uuid>()
        .map(BranchId::from_uuid)
        .map_err(|_| StateError::CorruptColumn("branch_id"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::StateError;
    use cyrene_core::{BranchId, SessionId};

    fn test_store() -> StateStore {
        StateStore::open_in_memory().unwrap()
    }

    #[test]
    fn round_trip_checkpoint_get_after_store() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();
        let mem = b"agent memory state";
        let vars = b"session variables";
        let files: &[(&str, &[u8])] =
            &[("src/main.rs", b"fn main() {}"), ("README.md", b"# Hello")];

        let id = store
            .checkpoint(
                session,
                0,
                None,
                branch,
                "initial checkpoint",
                mem,
                vars,
                files,
            )
            .unwrap();

        let cp = store.get(id).unwrap().expect("checkpoint should exist");
        assert_eq!(cp.id, id);
        assert_eq!(cp.session_id, session);
        assert_eq!(cp.step_seq, 0);
        assert_eq!(cp.parent_id, None);
        assert_eq!(cp.branch_id, branch);
        assert_eq!(cp.summary, "initial checkpoint");
        assert_eq!(cp.mem_blob, mem);
        assert_eq!(cp.vars_blob, vars);
        assert_eq!(cp.file_manifest.len(), 2);

        // Verify file content is retrievable via the manifest hashes.
        let main_hash = cp.file_manifest.get("src/main.rs").unwrap();
        let readme_hash = cp.file_manifest.get("README.md").unwrap();
        assert_eq!(
            store.read_blob(main_hash).unwrap().unwrap(),
            b"fn main() {}"
        );
        assert_eq!(store.read_blob(readme_hash).unwrap().unwrap(), b"# Hello");
    }

    #[test]
    fn content_dedup_same_bytes_one_blob_row() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();
        let content = b"shared content";

        // Two checkpoints referencing the same file content.
        let _id1 = store
            .checkpoint(
                session,
                0,
                None,
                branch,
                "step 0",
                b"",
                b"",
                &[("a.txt", content.as_slice())],
            )
            .unwrap();
        let _id2 = store
            .checkpoint(
                session,
                1,
                None,
                branch,
                "step 1",
                b"",
                b"",
                &[("b.txt", content.as_slice())],
            )
            .unwrap();

        // Only one blob row should exist despite two references.
        assert_eq!(store.blob_count().unwrap(), 1);
    }

    #[test]
    fn distinct_contents_produce_distinct_hashes() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();

        let id = store
            .checkpoint(
                session,
                0,
                None,
                branch,
                "step 0",
                b"",
                b"",
                &[("a.txt", b"content A"), ("b.txt", b"content B")],
            )
            .unwrap();

        let cp = store.get(id).unwrap().unwrap();
        let hash_a = cp.file_manifest.get("a.txt").unwrap();
        let hash_b = cp.file_manifest.get("b.txt").unwrap();
        assert_ne!(hash_a, hash_b);
        assert_eq!(store.blob_count().unwrap(), 2);
    }

    #[test]
    fn get_returns_none_for_unknown_id() {
        let store = test_store();
        let unknown = CheckpointId::new();
        assert!(store.get(unknown).unwrap().is_none());
    }

    #[test]
    fn read_blob_returns_none_for_unknown_hash() {
        let store = test_store();
        let unknown_hash = [0xABu8; 32];
        assert!(store.read_blob(&unknown_hash).unwrap().is_none());
    }

    #[test]
    fn checkpoint_with_parent_id_round_trips() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();

        let id1 = store
            .checkpoint(session, 0, None, branch, "first", b"m1", b"v1", &[])
            .unwrap();
        let id2 = store
            .checkpoint(session, 1, Some(id1), branch, "second", b"m2", b"v2", &[])
            .unwrap();

        let cp2 = store.get(id2).unwrap().unwrap();
        assert_eq!(cp2.parent_id, Some(id1));
        assert_eq!(cp2.step_seq, 1);
    }

    #[test]
    fn sha256_produces_correct_hash() {
        // Known SHA-256 of empty input.
        let empty_hash = sha256(b"");
        let expected: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(empty_hash, expected);
    }

    #[test]
    fn empty_file_manifest_round_trips() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();

        let id = store
            .checkpoint(session, 0, None, branch, "no files", b"mem", b"vars", &[])
            .unwrap();

        let cp = store.get(id).unwrap().unwrap();
        assert!(cp.file_manifest.is_empty());
    }

    // --- Task 5.2 tests ---

    #[test]
    fn checkout_returns_full_checkpoint_data() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();
        let mem = b"agent memory";
        let vars = b"session vars";
        let files: &[(&str, &[u8])] = &[("app.rs", b"fn app() {}")];

        let id = store
            .checkpoint(session, 0, None, branch, "step zero", mem, vars, files)
            .unwrap();

        let cp = store.checkout(id).unwrap();
        assert_eq!(cp.id, id);
        assert_eq!(cp.session_id, session);
        assert_eq!(cp.step_seq, 0);
        assert_eq!(cp.branch_id, branch);
        assert_eq!(cp.summary, "step zero");
        assert_eq!(cp.mem_blob, mem);
        assert_eq!(cp.vars_blob, vars);
        assert_eq!(cp.file_manifest.len(), 1);

        // Verify file content is retrievable via the manifest hash.
        let hash = cp.file_manifest.get("app.rs").unwrap();
        assert_eq!(store.read_blob(hash).unwrap().unwrap(), b"fn app() {}");
    }

    #[test]
    fn checkout_unknown_id_returns_error_listing_valid_ids() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();

        let id1 = store
            .checkpoint(session, 0, None, branch, "first", b"", b"", &[])
            .unwrap();
        let id2 = store
            .checkpoint(session, 1, Some(id1), branch, "second", b"", b"", &[])
            .unwrap();

        let unknown = CheckpointId::new();
        let err = store.checkout(unknown).unwrap_err();

        let error_msg = err.to_string();
        assert!(error_msg.contains(&unknown.to_string()));
        assert!(error_msg.contains(&id1.to_string()));
        assert!(error_msg.contains(&id2.to_string()));

        // Verify it's the NotFound variant with available ids.
        match err {
            StateError::NotFound { id, available } => {
                assert_eq!(id, unknown.to_string());
                assert!(available.contains(&id1.to_string()));
                assert!(available.contains(&id2.to_string()));
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[test]
    fn history_returns_ordered_summaries() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();

        let id0 = store
            .checkpoint(session, 0, None, branch, "init", b"", b"", &[])
            .unwrap();
        let id1 = store
            .checkpoint(session, 1, Some(id0), branch, "step one", b"", b"", &[])
            .unwrap();
        let id2 = store
            .checkpoint(session, 2, Some(id1), branch, "step two", b"", b"", &[])
            .unwrap();

        let history = store.history(session).unwrap();
        assert_eq!(history.len(), 3);

        assert_eq!(history[0].id, id0);
        assert_eq!(history[0].step_seq, 0);
        assert_eq!(history[0].summary, "init");
        assert_eq!(history[0].branch_id, branch);

        assert_eq!(history[1].id, id1);
        assert_eq!(history[1].step_seq, 1);
        assert_eq!(history[1].summary, "step one");

        assert_eq!(history[2].id, id2);
        assert_eq!(history[2].step_seq, 2);
        assert_eq!(history[2].summary, "step two");
    }

    #[test]
    fn history_returns_empty_for_unknown_session() {
        let store = test_store();
        let unknown_session = SessionId::new();
        let history = store.history(unknown_session).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn fork_branch_returns_fresh_id_distinct_from_original() {
        let store = test_store();
        let session = SessionId::new();
        let original_branch = BranchId::new();

        let id = store
            .checkpoint(
                session,
                0,
                None,
                original_branch,
                "checkpoint",
                b"mem",
                b"vars",
                &[],
            )
            .unwrap();

        let new_branch = store.fork_branch(id).unwrap();
        assert_ne!(new_branch, original_branch);
    }

    #[test]
    fn fork_branch_unknown_checkpoint_returns_error() {
        let store = test_store();
        let unknown = CheckpointId::new();
        let err = store.fork_branch(unknown).unwrap_err();
        match err {
            StateError::NotFound { id, .. } => {
                assert_eq!(id, unknown.to_string());
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[test]
    fn superseded_checkpoints_remain_after_forking() {
        let store = test_store();
        let session = SessionId::new();
        let branch = BranchId::new();

        // Create a chain of checkpoints on the original branch.
        let id0 = store
            .checkpoint(session, 0, None, branch, "step 0", b"m0", b"v0", &[])
            .unwrap();
        let id1 = store
            .checkpoint(session, 1, Some(id0), branch, "step 1", b"m1", b"v1", &[])
            .unwrap();
        let id2 = store
            .checkpoint(session, 2, Some(id1), branch, "step 2", b"m2", b"v2", &[])
            .unwrap();

        // Simulate checkout of step 1 and fork.
        let _restored = store.checkout(id1).unwrap();
        let new_branch = store.fork_branch(id1).unwrap();

        // Create a new checkpoint on the forked branch.
        let id3 = store
            .checkpoint(
                session,
                3,
                Some(id1),
                new_branch,
                "forked step",
                b"m3",
                b"v3",
                &[],
            )
            .unwrap();

        // All original checkpoints still exist (append-only invariant).
        assert!(store.get(id0).unwrap().is_some());
        assert!(store.get(id1).unwrap().is_some());
        assert!(store.get(id2).unwrap().is_some());
        assert!(store.get(id3).unwrap().is_some());

        // The superseded checkpoint (id2) is still on the original branch.
        let cp2 = store.get(id2).unwrap().unwrap();
        assert_eq!(cp2.branch_id, branch);

        // The new checkpoint is on the forked branch.
        let cp3 = store.get(id3).unwrap().unwrap();
        assert_eq!(cp3.branch_id, new_branch);
        assert_eq!(cp3.parent_id, Some(id1));
    }

    #[test]
    fn available_checkpoint_ids_returns_session_ids() {
        let store = test_store();
        let session = SessionId::new();
        let other_session = SessionId::new();
        let branch = BranchId::new();

        let id1 = store
            .checkpoint(session, 0, None, branch, "s1", b"", b"", &[])
            .unwrap();
        let id2 = store
            .checkpoint(session, 1, Some(id1), branch, "s2", b"", b"", &[])
            .unwrap();
        // Checkpoint in a different session — should not appear.
        let _other_id = store
            .checkpoint(other_session, 0, None, branch, "other", b"", b"", &[])
            .unwrap();

        let ids = store.available_checkpoint_ids(session).unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], id1);
        assert_eq!(ids[1], id2);
    }
}
