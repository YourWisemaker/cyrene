//! Property-based tests for the Receipt_Ledger Correctness Properties (task 4.3).
//!
//! These encode the two ledger invariants from the design's "Correctness
//! Properties" section, driven by `proptest` (the framework chosen in the
//! design's Testing Strategy):
//!
//! - **Property 1 — Ledger integrity (R5.2 / R5.3).** For any sequence of
//!   appended receipts, [`Ledger::verify`] returns
//!   [`LedgerVerification::Valid`]; and for any single-field mutation of any
//!   stored receipt, `verify()` detects *and localizes* the divergence (the
//!   reported `seq` is at or before the earliest tampered receipt). The hash
//!   chain stays unbroken (`receipt[n].prev_hash == receipt[n-1].hash`).
//! - **Property 2 — Append-only ordering (R5.4).** Receipts are strictly
//!   increasing in `seq` (0, 1, 2, … n-1), are never reordered or deleted, and
//!   the read-back order is exactly the append order.
//!
//! ## How the mutation generator works (Property 1b)
//!
//! A "stored receipt" is one SQLite row with columns `seq, session_id,
//! timestamp, action, inputs_digest, deciding_model, prev_hash, hash,
//! signature`. We mutate a persisted row through a *second* `rusqlite`
//! connection to the same DB file — the same persisted-tamper approach the 4.2
//! in-crate tests use — so `verify()` reads the tampered bytes back on its own
//! connection.
//!
//! The generator targets the columns that participate in the ledger's integrity
//! mechanism (the hash chain + signature), choosing an arbitrary receipt row and
//! an arbitrary mutation:
//!
//! - **Fixed-width binary columns** (`inputs_digest`, `prev_hash`, `hash`,
//!   `signature`): a literal *single-byte flip* — XOR a non-zero mask into an
//!   arbitrary byte position. The flip preserves the column's length (so the
//!   schema `CHECK` still holds) while guaranteeing the bytes actually change,
//!   and exercises every byte position of the 32-byte hashes/digest and the
//!   64-byte signature.
//! - **Hashed text/timestamp columns** (`action`, `deciding_model`,
//!   `timestamp`): a value alteration to a *different* valid value (a raw byte
//!   flip on a UTF-8 `TEXT` column would corrupt the encoding rather than model
//!   a realistic field tamper). The new value is forced to differ from the
//!   stored one, so the hashed content always changes.
//! - **`session_id`**: a reassignment to a *different* valid UUID. Because
//!   `session_id` is now bound into the chain hash, reattributing a stored
//!   receipt to another (well-formed) session changes the hashed content and so
//!   must be detected — closing the audit-integrity gap a prior revision left
//!   open (R5.1, R5.5).
//!
//! Every such mutation must make `verify()` return `Diverged` (never `Valid`),
//! localized at or before the mutated row.
//!
//! ### Scope note: `seq` is intentionally not in the mutation set
//!
//! `seq` is excluded because it is the structural primary key the ledger
//! assigns, not a free-form field a caller supplies; rewriting it models a row
//! relocation rather than a content tamper. Every other audited column —
//! including `session_id` — is now covered by a mutation kind above.

use chrono::{DateTime, Duration, SecondsFormat, TimeZone, Utc};
use cyrene_core::SessionId;
use cyrene_ledger::{digest_inputs, InstallKey, Ledger, LedgerVerification, ReceiptHash};
use proptest::prelude::*;
use proptest::sample::Index;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use uuid::Uuid;

/// Upper bound on generated receipts per case — kept modest so the suite stays
/// fast while still exercising multi-receipt chains.
const MAX_RECEIPTS: usize = 32;

/// Number of distinct sessions a generated run spreads its receipts across, so
/// the properties hold for interleaved multi-session ledgers (R5.5 read path).
const SESSION_POOL: usize = 3;

/// proptest cases per property. Smaller than the default 256 because each case
/// builds a fresh file-backed SQLite ledger and an ed25519 install key.
const CASES: u32 = 64;

/// A fixed base instant for deterministic, reproducible receipt timestamps
/// (2023-11-14T22:13:20Z). Using an explicit timestamp via `append_at` keeps the
/// chain hashes reproducible and the tests free of wall-clock flakiness.
fn base_time() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).unwrap()
}

/// The generated inputs for one appended receipt.
#[derive(Debug, Clone)]
struct ReceiptSpec {
    /// Which session in the pool this receipt belongs to.
    session_idx: usize,
    /// Arbitrary action description (no NUL, so it round-trips through `TEXT`).
    action: String,
    /// Arbitrary raw input bytes (digested before storage, never stored raw).
    inputs: Vec<u8>,
    /// Arbitrary deciding-model alias (no NUL).
    deciding_model: String,
}

/// Strategy for a single receipt: arbitrary session, action, input bytes, model.
fn receipt_spec() -> impl Strategy<Value = ReceiptSpec> {
    (
        0..SESSION_POOL,
        "[^\u{0}]{0,48}",
        prop::collection::vec(any::<u8>(), 0..64),
        "[^\u{0}]{0,32}",
    )
        .prop_map(
            |(session_idx, action, inputs, deciding_model)| ReceiptSpec {
                session_idx,
                action,
                inputs,
                deciding_model,
            },
        )
}

/// Strategy for a non-empty, bounded sequence of receipts to append.
fn receipt_specs() -> impl Strategy<Value = Vec<ReceiptSpec>> {
    prop::collection::vec(receipt_spec(), 1..=MAX_RECEIPTS)
}

/// A file-backed ledger plus the context a test needs to tamper with it.
struct Harness {
    ledger: Ledger,
    db_path: PathBuf,
    sessions: Vec<SessionId>,
    /// Kept alive so the temp dir (DB + key file) is not deleted mid-test.
    _dir: TempDir,
}

/// Builds a fresh file-backed ledger in a temp dir and appends `specs` with
/// deterministic, strictly increasing timestamps.
fn build_and_append(specs: &[ReceiptSpec]) -> Harness {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("cyrene.db");
    let key_path = InstallKey::default_path_in(dir.path());
    let ledger = Ledger::open(&db_path, &key_path).expect("open ledger");

    // Deterministic session ids derived from the pool index so a failing case
    // reproduces exactly from its proptest seed.
    let sessions: Vec<SessionId> = (0..SESSION_POOL)
        .map(|i| SessionId::from_uuid(Uuid::from_u128((i as u128) + 1)))
        .collect();

    for (i, spec) in specs.iter().enumerate() {
        let ts = base_time() + Duration::seconds(i as i64);
        ledger
            .append_at(
                sessions[spec.session_idx],
                spec.action.clone(),
                digest_inputs(&spec.inputs),
                spec.deciding_model.clone(),
                ts,
            )
            .expect("append receipt");
    }

    Harness {
        ledger,
        db_path,
        sessions,
        _dir: dir,
    }
}

/// A single-field tamper applied to one stored receipt row.
#[derive(Debug, Clone)]
enum Mutation {
    /// Flip one byte (XOR a non-zero mask) of a fixed-width binary column.
    BlobByteFlip {
        column: &'static str,
        pos: usize,
        mask: u8,
    },
    /// Replace `action` with a different valid string.
    NewAction(String),
    /// Replace `deciding_model` with a different valid string.
    NewModel(String),
    /// Reassign `session_id` to a different valid UUID.
    NewSession(u128),
    /// Shift `timestamp` by a non-zero number of seconds (a different instant).
    TimeShiftSecs(i64),
}

/// The fixed-width binary columns a single-byte flip can target. All are bound
/// into either the chain hash or the signature, so any flip must be detected.
const BLOB_COLUMNS: [&str; 4] = ["inputs_digest", "prev_hash", "hash", "signature"];

/// Strategy over the mutation kinds, exercising binary single-byte flips and
/// hashed text/timestamp value changes.
fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (
            prop::sample::select(BLOB_COLUMNS.as_slice()),
            0usize..64,
            1u8..=u8::MAX
        )
            .prop_map(|(column, pos, mask)| Mutation::BlobByteFlip { column, pos, mask }),
        "[^\u{0}]{0,48}".prop_map(Mutation::NewAction),
        "[^\u{0}]{0,32}".prop_map(Mutation::NewModel),
        any::<u128>().prop_map(Mutation::NewSession),
        (1i64..=100_000)
            .prop_flat_map(|m| prop_oneof![Just(m), Just(-m)])
            .prop_map(Mutation::TimeShiftSecs),
    ]
}

/// Applies `mutation` to the row at `seq` through a second connection to the
/// same DB file, persisting the tamper for `verify()` to detect.
fn apply_mutation(db_path: &Path, seq: u64, mutation: &Mutation) {
    let conn = Connection::open(db_path).expect("second connection");
    match mutation {
        Mutation::BlobByteFlip { column, pos, mask } => {
            let read = format!("SELECT {column} FROM receipts WHERE seq = ?1");
            let mut bytes: Vec<u8> = conn
                .query_row(&read, [seq as i64], |row| row.get(0))
                .expect("read blob column");
            // A non-zero mask flips at least one bit, so the bytes always change;
            // the modulo keeps the position in range for 32- or 64-byte columns.
            let at = pos % bytes.len();
            bytes[at] ^= mask;
            let write = format!("UPDATE receipts SET {column} = ?1 WHERE seq = ?2");
            conn.execute(&write, params![bytes, seq as i64])
                .expect("write blob column");
        }
        Mutation::NewAction(candidate) => {
            let current: String = conn
                .query_row(
                    "SELECT action FROM receipts WHERE seq = ?1",
                    [seq as i64],
                    |row| row.get(0),
                )
                .expect("read action");
            let new = different_value(candidate, &current);
            conn.execute(
                "UPDATE receipts SET action = ?1 WHERE seq = ?2",
                params![new, seq as i64],
            )
            .expect("write action");
        }
        Mutation::NewModel(candidate) => {
            let current: String = conn
                .query_row(
                    "SELECT deciding_model FROM receipts WHERE seq = ?1",
                    [seq as i64],
                    |row| row.get(0),
                )
                .expect("read deciding_model");
            let new = different_value(candidate, &current);
            conn.execute(
                "UPDATE receipts SET deciding_model = ?1 WHERE seq = ?2",
                params![new, seq as i64],
            )
            .expect("write deciding_model");
        }
        Mutation::NewSession(candidate) => {
            let current: String = conn
                .query_row(
                    "SELECT session_id FROM receipts WHERE seq = ?1",
                    [seq as i64],
                    |row| row.get(0),
                )
                .expect("read session_id");
            // Build a valid UUID from the candidate; if it happens to equal the
            // stored one, perturb it so the reassignment always targets a
            // *different* (still well-formed) session.
            let mut new_uuid = Uuid::from_u128(*candidate);
            if new_uuid.to_string() == current {
                new_uuid = Uuid::from_u128(candidate.wrapping_add(1));
            }
            conn.execute(
                "UPDATE receipts SET session_id = ?1 WHERE seq = ?2",
                params![new_uuid.to_string(), seq as i64],
            )
            .expect("write session_id");
        }
        Mutation::TimeShiftSecs(offset) => {
            let current: String = conn
                .query_row(
                    "SELECT timestamp FROM receipts WHERE seq = ?1",
                    [seq as i64],
                    |row| row.get(0),
                )
                .expect("read timestamp");
            let parsed = DateTime::parse_from_rfc3339(&current)
                .expect("stored timestamp is rfc3339")
                .with_timezone(&Utc);
            // A non-zero second offset guarantees a different instant, so the
            // recomputed hash must differ.
            let shifted = parsed + Duration::seconds(*offset);
            let new = shifted.to_rfc3339_opts(SecondsFormat::Nanos, true);
            conn.execute(
                "UPDATE receipts SET timestamp = ?1 WHERE seq = ?2",
                params![new, seq as i64],
            )
            .expect("write timestamp");
        }
    }
}

/// Returns `candidate` if it differs from `current`, otherwise a guaranteed
/// different string, so a value mutation always changes the stored bytes.
fn different_value(candidate: &str, current: &str) -> String {
    if candidate != current {
        candidate.to_owned()
    } else {
        format!("{candidate}\u{1}tamper")
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(CASES))]

    /// Property 1a: any appended sequence verifies as `Valid`.
    ///
    /// **Validates: Requirements 5.2, 5.3**
    #[test]
    fn prop1a_any_appended_sequence_verifies(specs in receipt_specs()) {
        let harness = build_and_append(&specs);

        prop_assert_eq!(harness.ledger.verify().expect("verify"), LedgerVerification::Valid);
        prop_assert_eq!(harness.ledger.len().expect("len"), specs.len() as u64);
    }

    /// Property 1b: any single-field mutation of any stored receipt is detected
    /// and localized — `verify()` returns `Diverged` (never `Valid`) at a `seq`
    /// at or before the mutated row, and an intact ledger verifies first.
    ///
    /// **Validates: Requirements 5.2, 5.3**
    #[test]
    fn prop1b_single_field_mutation_is_detected_and_localized(
        specs in receipt_specs(),
        target in any::<Index>(),
        mutation in mutation(),
    ) {
        let harness = build_and_append(&specs);
        let r = target.index(specs.len()) as u64;

        // The freshly built chain verifies before tampering.
        prop_assert_eq!(harness.ledger.verify().expect("verify intact"), LedgerVerification::Valid);

        apply_mutation(&harness.db_path, r, &mutation);

        let verification = harness.ledger.verify().expect("verify after tamper");
        prop_assert!(
            !verification.is_valid(),
            "mutation {:?} at seq {} was not detected",
            mutation,
            r
        );
        match verification {
            LedgerVerification::Diverged { seq, .. } => {
                prop_assert!(
                    seq <= r,
                    "divergence localized at seq {seq}, expected at or before {r}"
                );
            }
            LedgerVerification::Valid => unreachable!("guarded by the assertion above"),
        }
    }

    /// Property 2: `seq` is strictly increasing and append-only — read-back is
    /// `0, 1, … n-1`, the chain links hold, and the order matches append order.
    ///
    /// `Ledger` exposes no update or delete method (only `append`/`append_at`),
    /// so demonstrating that the read-back is exactly the appended sequence, in
    /// order, is the observable append-only guarantee.
    ///
    /// **Validates: Requirements 5.4**
    #[test]
    fn prop2_seq_is_strictly_increasing_and_append_only(specs in receipt_specs()) {
        let harness = build_and_append(&specs);
        let stored = harness.ledger.all_receipts().expect("all_receipts");

        // Nothing is dropped or added: one stored receipt per appended spec.
        prop_assert_eq!(stored.len(), specs.len());

        // seq is exactly 0, 1, 2, …, n-1.
        for (i, receipt) in stored.iter().enumerate() {
            prop_assert_eq!(receipt.seq, i as u64);
        }

        // The genesis receipt links to the all-zero hash.
        prop_assert_eq!(stored[0].prev_hash, ReceiptHash::ZERO);

        // Strictly increasing by exactly one, with an unbroken hash chain.
        for window in stored.windows(2) {
            prop_assert!(window[0].seq < window[1].seq);
            prop_assert_eq!(window[1].seq, window[0].seq + 1);
            prop_assert_eq!(window[1].prev_hash, window[0].hash);
        }

        // Read-back order is the append order: the i-th stored receipt carries
        // the i-th appended spec's action and session (never reordered).
        for (i, spec) in specs.iter().enumerate() {
            prop_assert_eq!(&stored[i].action, &spec.action);
            prop_assert_eq!(stored[i].session_id, harness.sessions[spec.session_idx]);
        }

        // Per-session reads are also strictly increasing in seq (R5.5).
        for session in &harness.sessions {
            let per_session = harness
                .ledger
                .receipts_for_session(*session)
                .expect("receipts_for_session");
            for window in per_session.windows(2) {
                prop_assert!(window[0].seq < window[1].seq);
            }
            prop_assert!(per_session.iter().all(|r| r.session_id == *session));
        }
    }
}
