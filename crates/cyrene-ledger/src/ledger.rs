//! The append-only, signed, hash-chained Receipt_Ledger backed by SQLite.
//!
//! [`Ledger`] owns a `rusqlite` connection (embedded SQLite via the `bundled`
//! feature, so no system SQLite is required) and an [`InstallKey`] for signing.
//! Opening a ledger initializes the schema if absent. [`Ledger::append`]
//! records one [`ToolReceipt`]: it links `prev_hash` to the last receipt's
//! `hash` (or [`ReceiptHash::ZERO`] for the genesis receipt), computes the
//! chain hash, signs it with the install key, and inserts the row.
//!
//! The append API is the *only* write path; there is no update/delete method,
//! and `seq` is assigned monotonically by the ledger so callers cannot reorder
//! or overwrite history (R5.4). Task 4.2 adds `verify()` and per-session query
//! on top of this, reusing [`compute_hash`](crate::receipt::compute_hash) and
//! the keypair's verification primitive.

use std::path::Path;

use chrono::{DateTime, TimeZone, Utc};
use cyrene_core::SessionId;
use rusqlite::{Connection, OptionalExtension};

use crate::error::LedgerError;
use crate::keypair::InstallKey;
use crate::receipt::{
    compute_hash, InputsDigest, ReceiptContent, ReceiptHash, ToolReceipt, HASH_LEN, SIGNATURE_LEN,
};
use crate::verify::{DivergenceKind, LedgerVerification};

/// The schema for the append-only receipts table.
///
/// `seq` is the primary key and rows are read back ordered by it (R5.4). A
/// `CHECK` constraint pins the hash/signature column widths so a malformed blob
/// cannot be inserted. The genesis receipt stores an all-zero `prev_hash`.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS receipts (
    seq            INTEGER PRIMARY KEY,
    session_id     TEXT    NOT NULL,
    timestamp      TEXT    NOT NULL,
    action         TEXT    NOT NULL,
    inputs_digest  BLOB    NOT NULL CHECK (length(inputs_digest) = 32),
    deciding_model TEXT    NOT NULL,
    prev_hash      BLOB    NOT NULL CHECK (length(prev_hash) = 32),
    hash           BLOB    NOT NULL CHECK (length(hash) = 32),
    signature      BLOB    NOT NULL CHECK (length(signature) = 64)
);
CREATE INDEX IF NOT EXISTS idx_receipts_session ON receipts (session_id, seq);";

/// An append-only, signed Receipt_Ledger.
#[derive(Debug)]
pub struct Ledger {
    conn: Connection,
    key: InstallKey,
}

impl Ledger {
    /// Opens (or creates) a ledger at `db_path`, using the install key at
    /// `key_path` (generated and persisted if it does not yet exist).
    ///
    /// The schema is initialized on open if absent. Pass an explicit path so
    /// tests can target a temp file; production wiring uses
    /// `~/.cyrene/cyrene.db` and [`InstallKey::default_path_in`].
    ///
    /// # Errors
    /// Returns [`LedgerError`] if the database cannot be opened/initialized or
    /// the install key cannot be loaded/generated.
    pub fn open(
        db_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, LedgerError> {
        let conn = Connection::open(db_path)?;
        let key = InstallKey::load_or_generate(key_path)?;
        Self::from_parts(conn, key)
    }

    /// Opens an in-memory ledger with the given install key. Intended for tests.
    ///
    /// # Errors
    /// Returns [`LedgerError`] if the schema cannot be initialized.
    pub fn open_in_memory(key: InstallKey) -> Result<Self, LedgerError> {
        let conn = Connection::open_in_memory()?;
        Self::from_parts(conn, key)
    }

    /// Initializes the schema and wraps the connection + key into a [`Ledger`].
    fn from_parts(conn: Connection, key: InstallKey) -> Result<Self, LedgerError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn, key })
    }

    /// Borrows the install key (its public verifying key is what task 4.2 uses
    /// to verify signatures).
    #[must_use]
    pub fn install_key(&self) -> &InstallKey {
        &self.key
    }

    /// Appends a receipt for one action and returns the stored [`ToolReceipt`].
    ///
    /// `seq` is assigned as `last_seq + 1` (or `0` for the first receipt),
    /// `prev_hash` is the previous receipt's `hash` (or [`ReceiptHash::ZERO`]),
    /// the chain `hash` is computed via
    /// [`compute_hash`](crate::receipt::compute_hash), and the signature is
    /// `ed25519(secret_key, hash)`. The timestamp defaults to [`Utc::now`].
    ///
    /// Inputs must already be digested by the caller (see
    /// [`digest_inputs`](crate::receipt::digest_inputs)) so raw, possibly
    /// secret-bearing inputs are never written.
    ///
    /// # Errors
    /// Returns [`LedgerError`] on a database failure or if `seq` would overflow.
    pub fn append(
        &self,
        session_id: SessionId,
        action: impl Into<String>,
        inputs_digest: InputsDigest,
        deciding_model: impl Into<String>,
    ) -> Result<ToolReceipt, LedgerError> {
        self.append_at(
            session_id,
            action,
            inputs_digest,
            deciding_model,
            Utc::now(),
        )
    }

    /// Like [`append`](Self::append) but with an explicit timestamp, so tests
    /// can pin time and the hash is reproducible.
    ///
    /// # Errors
    /// Returns [`LedgerError`] on a database failure or if `seq` would overflow.
    pub fn append_at(
        &self,
        session_id: SessionId,
        action: impl Into<String>,
        inputs_digest: InputsDigest,
        deciding_model: impl Into<String>,
        timestamp: DateTime<Utc>,
    ) -> Result<ToolReceipt, LedgerError> {
        let action = action.into();
        let deciding_model = deciding_model.into();

        let (prev_hash, seq) = match self.last_seq_and_hash()? {
            Some((last_seq, last_hash)) => {
                let next = last_seq.checked_add(1).ok_or(LedgerError::SeqOverflow)?;
                (last_hash, next)
            }
            None => (ReceiptHash::ZERO, 0),
        };

        let hash = compute_hash(&ReceiptContent {
            seq,
            session_id,
            timestamp,
            action: &action,
            inputs_digest,
            deciding_model: &deciding_model,
            prev_hash,
        });
        let signature = self.key.sign(&hash);

        self.conn.execute(
            "INSERT INTO receipts
                (seq, session_id, timestamp, action, inputs_digest,
                 deciding_model, prev_hash, hash, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                seq,
                session_id.to_string(),
                encode_timestamp(timestamp),
                action,
                inputs_digest.as_slice(),
                deciding_model,
                prev_hash.as_slice(),
                hash.as_slice(),
                signature.as_slice(),
            ],
        )?;

        Ok(ToolReceipt {
            seq,
            session_id,
            timestamp,
            action,
            inputs_digest,
            deciding_model,
            prev_hash,
            hash,
            signature,
        })
    }

    /// Returns the number of receipts in the ledger.
    ///
    /// # Errors
    /// Returns [`LedgerError`] on a database failure.
    pub fn len(&self) -> Result<u64, LedgerError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    /// Returns `true` if the ledger holds no receipts.
    ///
    /// # Errors
    /// Returns [`LedgerError`] on a database failure.
    pub fn is_empty(&self) -> Result<bool, LedgerError> {
        Ok(self.len()? == 0)
    }

    /// Reads every receipt back in append (`seq`) order (R5.4).
    ///
    /// Task 4.2 builds `verify()` and per-session queries on this read path.
    ///
    /// # Errors
    /// Returns [`LedgerError`] on a database failure or a corrupt stored row.
    pub fn all_receipts(&self) -> Result<Vec<ToolReceipt>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, session_id, timestamp, action, inputs_digest,
                    deciding_model, prev_hash, hash, signature
             FROM receipts
             ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map([], |row| Ok(row_to_receipt(row)))?;

        let mut receipts = Vec::new();
        for row in rows {
            receipts.push(row??);
        }
        Ok(receipts)
    }

    /// Verifies the integrity of the whole ledger and localizes the first
    /// divergence (R5.3).
    ///
    /// Walks every receipt in `seq` order and, for each, checks three
    /// invariants in priority order (design section 2):
    ///
    /// 1. the recomputed [`compute_hash`](crate::receipt::compute_hash) of the
    ///    receipt's content equals the stored `hash`
    ///    ([`DivergenceKind::HashMismatch`] on failure);
    /// 2. the chain link holds — `prev_hash` equals the previous receipt's
    ///    stored `hash`, or [`ReceiptHash::ZERO`] for the genesis receipt at
    ///    `seq` 0 ([`DivergenceKind::BrokenLink`] on failure);
    /// 3. the `signature` validates against the install verifying key for the
    ///    stored `hash` ([`DivergenceKind::BadSignature`] on failure).
    ///
    /// The walk stops at the first failing receipt and returns
    /// [`LedgerVerification::Diverged`] with that receipt's `seq` and the
    /// failing [`DivergenceKind`]; an empty or fully intact ledger returns
    /// [`LedgerVerification::Valid`].
    ///
    /// This is a read-only operation: it never writes to the ledger, preserving
    /// the append-only guarantee.
    ///
    /// # Errors
    /// Returns [`LedgerError`] on a database failure or a corrupt stored row.
    pub fn verify(&self) -> Result<LedgerVerification, LedgerError> {
        let verifying_key = self.key.verifying_key();
        let mut expected_prev = ReceiptHash::ZERO;

        for receipt in self.all_receipts()? {
            // 1. The stored hash must recompute from the receipt's content.
            let recomputed = compute_hash(&ReceiptContent {
                seq: receipt.seq,
                session_id: receipt.session_id,
                timestamp: receipt.timestamp,
                action: &receipt.action,
                inputs_digest: receipt.inputs_digest,
                deciding_model: &receipt.deciding_model,
                prev_hash: receipt.prev_hash,
            });
            if recomputed != receipt.hash {
                return Ok(LedgerVerification::diverged(
                    receipt.seq,
                    DivergenceKind::HashMismatch,
                ));
            }

            // 2. The chain link must hold (genesis links to ZERO).
            if receipt.prev_hash != expected_prev {
                return Ok(LedgerVerification::diverged(
                    receipt.seq,
                    DivergenceKind::BrokenLink,
                ));
            }

            // 3. The signature must validate against the install key.
            if !InstallKey::verify_signature(&verifying_key, &receipt.hash, &receipt.signature) {
                return Ok(LedgerVerification::diverged(
                    receipt.seq,
                    DivergenceKind::BadSignature,
                ));
            }

            // The next receipt must chain to this one's stored hash.
            expected_prev = receipt.hash;
        }

        Ok(LedgerVerification::Valid)
    }

    /// Returns every receipt for `session_id` in chronological (`seq` ascending)
    /// order (R5.5).
    ///
    /// Uses the `(session_id, seq)` index so the query is scoped to the session
    /// and already ordered. This is a read-only operation.
    ///
    /// # Errors
    /// Returns [`LedgerError`] on a database failure or a corrupt stored row.
    pub fn receipts_for_session(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<ToolReceipt>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, session_id, timestamp, action, inputs_digest,
                    deciding_model, prev_hash, hash, signature
             FROM receipts
             WHERE session_id = ?1
             ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map([session_id.to_string()], |row| Ok(row_to_receipt(row)))?;

        let mut receipts = Vec::new();
        for row in rows {
            receipts.push(row??);
        }
        Ok(receipts)
    }

    /// Returns the `(seq, hash)` of the most recently appended receipt, or
    /// [`None`] when the ledger is empty.
    fn last_seq_and_hash(&self) -> Result<Option<(u64, ReceiptHash)>, LedgerError> {
        let row = self
            .conn
            .query_row(
                "SELECT seq, hash FROM receipts ORDER BY seq DESC LIMIT 1",
                [],
                |row| {
                    let seq: i64 = row.get(0)?;
                    let hash: Vec<u8> = row.get(1)?;
                    Ok((seq, hash))
                },
            )
            .optional()?;

        match row {
            Some((seq, hash)) => Ok(Some((seq as u64, decode_hash(&hash, "hash")?))),
            None => Ok(None),
        }
    }
}

/// Encodes a timestamp for storage as an RFC 3339 string with nanosecond
/// precision (lossless, human-readable, sortable).
fn encode_timestamp(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

/// Parses a stored RFC 3339 timestamp back into a UTC datetime.
fn decode_timestamp(raw: &str) -> Result<DateTime<Utc>, LedgerError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| Utc.from_utc_datetime(&dt.naive_utc()))
        .map_err(|e| LedgerError::Timestamp(e.to_string()))
}

/// Decodes a 32-byte hash/digest column, erroring on a wrong length.
fn decode_hash(bytes: &[u8], column: &'static str) -> Result<ReceiptHash, LedgerError> {
    let arr: [u8; HASH_LEN] = bytes
        .try_into()
        .map_err(|_| LedgerError::CorruptColumn(column))?;
    Ok(ReceiptHash(arr))
}

/// Decodes a 32-byte inputs-digest column, erroring on a wrong length.
fn decode_inputs_digest(bytes: &[u8]) -> Result<InputsDigest, LedgerError> {
    let arr: [u8; HASH_LEN] = bytes
        .try_into()
        .map_err(|_| LedgerError::CorruptColumn("inputs_digest"))?;
    Ok(InputsDigest(arr))
}

/// Decodes a 64-byte signature column, erroring on a wrong length.
fn decode_signature(bytes: &[u8]) -> Result<[u8; SIGNATURE_LEN], LedgerError> {
    bytes
        .try_into()
        .map_err(|_| LedgerError::CorruptColumn("signature"))
}

/// Maps a SQLite row to a [`ToolReceipt`], validating column widths.
///
/// Returns a nested result: the outer `rusqlite::Result` covers raw column
/// access, the inner [`LedgerError`] covers decode/length validation.
fn row_to_receipt(row: &rusqlite::Row<'_>) -> Result<ToolReceipt, LedgerError> {
    let seq: i64 = row.get(0)?;
    let session_raw: String = row.get(1)?;
    let timestamp_raw: String = row.get(2)?;
    let action: String = row.get(3)?;
    let inputs_digest: Vec<u8> = row.get(4)?;
    let deciding_model: String = row.get(5)?;
    let prev_hash: Vec<u8> = row.get(6)?;
    let hash: Vec<u8> = row.get(7)?;
    let signature: Vec<u8> = row.get(8)?;

    let session_id = session_raw
        .parse::<uuid::Uuid>()
        .map(SessionId::from_uuid)
        .map_err(|_| LedgerError::CorruptColumn("session_id"))?;

    Ok(ToolReceipt {
        seq: seq as u64,
        session_id,
        timestamp: decode_timestamp(&timestamp_raw)?,
        action,
        inputs_digest: decode_inputs_digest(&inputs_digest)?,
        deciding_model,
        prev_hash: decode_hash(&prev_hash, "prev_hash")?,
        hash: decode_hash(&hash, "hash")?,
        signature: decode_signature(&signature)?,
    })
}

#[cfg(test)]
mod tests {
    use super::Ledger;
    use crate::keypair::InstallKey;
    use crate::receipt::digest_inputs;
    use crate::verify::{DivergenceKind, LedgerVerification};
    use cyrene_core::SessionId;
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};

    /// Opens a file-backed ledger in a fresh temp dir so a test can reopen the
    /// *same DB file* on a second connection to simulate real, persisted
    /// tampering that `verify()` must then detect.
    ///
    /// Returns the ledger, its DB path, and the temp-dir guard (kept alive so
    /// the files are not deleted while the test runs).
    fn file_ledger() -> (Ledger, PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cyrene.db");
        let key_path = InstallKey::default_path_in(dir.path());
        let ledger = Ledger::open(&db_path, &key_path).unwrap();
        (ledger, db_path, dir)
    }

    /// Tamper helper: opens a second connection to the same DB file and rewrites
    /// the `action` of the row at `seq` (so the stored hash no longer matches).
    fn tamper_set_action(db_path: &Path, seq: u64, action: &str) {
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "UPDATE receipts SET action = ?1 WHERE seq = ?2",
            rusqlite::params![action, seq as i64],
        )
        .unwrap();
    }

    /// Tamper helper: flips the first byte of a 32/64-byte blob `column`
    /// (`hash` or `signature`) of the row at `seq`, on a second connection.
    fn tamper_flip_first_byte(db_path: &Path, column: &str, seq: u64) {
        let conn = Connection::open(db_path).unwrap();
        let read_sql = format!("SELECT {column} FROM receipts WHERE seq = ?1");
        let mut bytes: Vec<u8> = conn
            .query_row(&read_sql, [seq as i64], |row| row.get(0))
            .unwrap();
        bytes[0] ^= 0x01;
        let write_sql = format!("UPDATE receipts SET {column} = ?1 WHERE seq = ?2");
        conn.execute(&write_sql, rusqlite::params![bytes, seq as i64])
            .unwrap();
    }

    /// Tamper helper: deletes the row at `seq` (simulating a removed receipt),
    /// on a second connection.
    fn tamper_delete_row(db_path: &Path, seq: u64) {
        let conn = Connection::open(db_path).unwrap();
        conn.execute("DELETE FROM receipts WHERE seq = ?1", [seq as i64])
            .unwrap();
    }

    /// Tamper helper: reassigns the `session_id` of the row at `seq` to a
    /// different valid UUID, on a second connection.
    fn tamper_set_session(db_path: &Path, seq: u64, session_id: SessionId) {
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "UPDATE receipts SET session_id = ?1 WHERE seq = ?2",
            rusqlite::params![session_id.to_string(), seq as i64],
        )
        .unwrap();
    }

    #[test]
    fn verify_is_valid_for_empty_ledger() {
        let (ledger, _db, _dir) = file_ledger();
        assert_eq!(ledger.verify().unwrap(), LedgerVerification::Valid);
    }

    #[test]
    fn verify_is_valid_for_an_intact_chain() {
        let (ledger, _db, _dir) = file_ledger();
        let session = SessionId::new();

        for i in 0..5 {
            ledger
                .append(
                    session,
                    format!("step-{i}"),
                    digest_inputs(format!("in-{i}")),
                    "local",
                )
                .unwrap();
        }

        let verification = ledger.verify().unwrap();
        assert_eq!(verification, LedgerVerification::Valid);
        assert!(verification.is_valid());
        assert_eq!(verification.divergence_seq(), None);
    }

    #[test]
    fn receipts_for_session_returns_only_that_session_in_chronological_order() {
        let (ledger, _db, _dir) = file_ledger();
        let session_a = SessionId::new();
        let session_b = SessionId::new();

        // Interleave appends across two sessions; seq is assigned globally.
        let a0 = ledger
            .append(session_a, "a0", digest_inputs("a0"), "local")
            .unwrap();
        let b0 = ledger
            .append(session_b, "b0", digest_inputs("b0"), "local")
            .unwrap();
        let a1 = ledger
            .append(session_a, "a1", digest_inputs("a1"), "local")
            .unwrap();
        let b1 = ledger
            .append(session_b, "b1", digest_inputs("b1"), "local")
            .unwrap();
        let a2 = ledger
            .append(session_a, "a2", digest_inputs("a2"), "local")
            .unwrap();

        let for_a = ledger.receipts_for_session(session_a).unwrap();
        // Only session A's receipts, in chronological (seq ascending) order.
        assert_eq!(for_a, vec![a0, a1, a2]);
        assert!(for_a.iter().all(|r| r.session_id == session_a));
        assert!(for_a.windows(2).all(|w| w[0].seq < w[1].seq));

        let for_b = ledger.receipts_for_session(session_b).unwrap();
        assert_eq!(for_b, vec![b0, b1]);
        assert!(for_b.iter().all(|r| r.session_id == session_b));
    }

    #[test]
    fn receipts_for_session_is_empty_for_an_unknown_session() {
        let (ledger, _db, _dir) = file_ledger();
        ledger
            .append(SessionId::new(), "x", digest_inputs("x"), "local")
            .unwrap();

        let other = SessionId::new();
        assert!(ledger.receipts_for_session(other).unwrap().is_empty());
    }

    #[test]
    fn verify_localizes_a_tampered_action_as_hash_mismatch() {
        let (ledger, db, _dir) = file_ledger();
        let session = SessionId::new();
        for i in 0..3 {
            ledger
                .append(session, format!("s{i}"), digest_inputs("x"), "local")
                .unwrap();
        }

        // Rewrite the action of the middle receipt: its content no longer
        // hashes to the stored `hash`.
        tamper_set_action(&db, 1, "rewritten-action");

        assert_eq!(
            ledger.verify().unwrap(),
            LedgerVerification::Diverged {
                seq: 1,
                kind: DivergenceKind::HashMismatch,
            }
        );
    }

    #[test]
    fn verify_localizes_a_tampered_stored_hash_as_hash_mismatch() {
        let (ledger, db, _dir) = file_ledger();
        let session = SessionId::new();
        for i in 0..3 {
            ledger
                .append(session, format!("s{i}"), digest_inputs("x"), "local")
                .unwrap();
        }

        // Flip a byte of the stored hash of receipt 0: it no longer matches the
        // hash recomputed from its (intact) content.
        tamper_flip_first_byte(&db, "hash", 0);

        assert_eq!(
            ledger.verify().unwrap(),
            LedgerVerification::Diverged {
                seq: 0,
                kind: DivergenceKind::HashMismatch,
            }
        );
    }

    #[test]
    fn verify_localizes_a_tampered_signature_as_bad_signature() {
        let (ledger, db, _dir) = file_ledger();
        let session = SessionId::new();
        for i in 0..3 {
            ledger
                .append(session, format!("s{i}"), digest_inputs("x"), "local")
                .unwrap();
        }

        // Flipping a signature byte leaves the hash and chain link intact, so
        // verification reaches — and fails — the signature check.
        tamper_flip_first_byte(&db, "signature", 2);

        assert_eq!(
            ledger.verify().unwrap(),
            LedgerVerification::Diverged {
                seq: 2,
                kind: DivergenceKind::BadSignature,
            }
        );
    }

    #[test]
    fn verify_localizes_a_reassigned_session_as_hash_mismatch() {
        let (ledger, db, _dir) = file_ledger();
        let session = SessionId::new();
        for i in 0..3 {
            ledger
                .append(session, format!("s{i}"), digest_inputs("x"), "local")
                .unwrap();
        }

        // Reassign the middle receipt to a *different* valid session. Since
        // session_id is bound into the hash, the stored hash no longer matches
        // the content recomputed from the reassigned session.
        let other = SessionId::new();
        assert_ne!(other, session);
        tamper_set_session(&db, 1, other);

        assert_eq!(
            ledger.verify().unwrap(),
            LedgerVerification::Diverged {
                seq: 1,
                kind: DivergenceKind::HashMismatch,
            }
        );
    }

    #[test]
    fn verify_localizes_a_removed_receipt_as_broken_link() {
        let (ledger, db, _dir) = file_ledger();
        let session = SessionId::new();
        for i in 0..3 {
            ledger
                .append(session, format!("s{i}"), digest_inputs("x"), "local")
                .unwrap();
        }

        // Remove the middle receipt. Receipt 2's own hash still recomputes (its
        // content is intact), but its `prev_hash` now points at the removed
        // receipt's hash, so the chain link to receipt 0 is broken.
        tamper_delete_row(&db, 1);

        assert_eq!(
            ledger.verify().unwrap(),
            LedgerVerification::Diverged {
                seq: 2,
                kind: DivergenceKind::BrokenLink,
            }
        );
    }
}
