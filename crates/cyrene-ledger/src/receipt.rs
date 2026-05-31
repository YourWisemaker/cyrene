//! The [`ToolReceipt`] model and the hash-chaining primitives.
//!
//! A `Tool_Receipt` is one signed, immutable record of a single action (R5.1).
//! The ledger is a hash chain: each receipt's [`hash`](ToolReceipt::hash) is
//!
//! ```text
//! SHA256(seq || session_id || timestamp || action || inputs_digest || deciding_model || prev_hash)
//! ```
//!
//! and `prev_hash` links to the previous receipt's `hash` (a zero genesis hash
//! for the first receipt). Any retroactive edit changes a hash and breaks the
//! chain (R5.2). The `signature` is `ed25519(secret_key, hash)`.
//!
//! Inputs are stored only as a digest — never as the raw bytes — so the ledger
//! never persists secret-bearing tool arguments verbatim (design section 2).
//! Use [`digest_inputs`] / [`InputsDigest::of`] to compute that digest.

use chrono::{DateTime, Utc};
use cyrene_core::SessionId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The byte length of a SHA-256 hash and of [`ReceiptHash`] / [`InputsDigest`].
pub const HASH_LEN: usize = 32;

/// The byte length of an ed25519 signature.
pub const SIGNATURE_LEN: usize = 64;

/// A 32-byte SHA-256 chain hash (a receipt's `hash` or `prev_hash`).
///
/// The genesis `prev_hash` for the first receipt is all zeroes
/// ([`ReceiptHash::ZERO`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReceiptHash(pub [u8; HASH_LEN]);

impl ReceiptHash {
    /// The all-zero genesis hash used as the first receipt's `prev_hash`.
    pub const ZERO: Self = Self([0u8; HASH_LEN]);

    /// Returns the raw 32 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; HASH_LEN] {
        &self.0
    }

    /// Borrows the hash as a byte slice (for SQLite binding).
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

/// A 32-byte SHA-256 digest of a step's inputs.
///
/// The ledger stores this digest in place of the raw inputs so secret-bearing
/// arguments are never persisted verbatim (design section 2, R5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InputsDigest(pub [u8; HASH_LEN]);

impl InputsDigest {
    /// Computes the SHA-256 digest of arbitrary input bytes.
    ///
    /// Callers pass the canonical serialized form of the step's inputs; the
    /// digest is what gets stored, so the raw (possibly secret) bytes never
    /// touch the ledger.
    #[must_use]
    pub fn of(inputs: impl AsRef<[u8]>) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(inputs.as_ref());
        Self(hasher.finalize().into())
    }

    /// Returns the raw 32 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; HASH_LEN] {
        &self.0
    }

    /// Borrows the digest as a byte slice (for SQLite binding).
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

/// Computes the inputs digest for a step's serialized inputs.
///
/// Thin free-function wrapper over [`InputsDigest::of`] so callers can write
/// `digest_inputs(bytes)` at the call site.
#[must_use]
pub fn digest_inputs(inputs: impl AsRef<[u8]>) -> InputsDigest {
    InputsDigest::of(inputs)
}

/// A single signed, immutable record of one action in the Receipt_Ledger.
///
/// Receipts are append-only and ordered by [`seq`](ToolReceipt::seq) (R5.4).
/// The fields fed into the hash are exactly those listed in the design:
/// `seq`, `session_id`, `timestamp`, `action`, `inputs_digest`,
/// `deciding_model`, `prev_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolReceipt {
    /// Monotonic sequence number; strictly increasing, append-only (R5.4).
    pub seq: u64,
    /// The session this action belongs to.
    pub session_id: SessionId,
    /// When the action was recorded (UTC).
    pub timestamp: DateTime<Utc>,
    /// A description of the action that was performed (R5.1).
    pub action: String,
    /// SHA-256 digest of the action's inputs — never the raw inputs (R5.1).
    pub inputs_digest: InputsDigest,
    /// The Model_Provider alias that decided this action (R5.1).
    pub deciding_model: String,
    /// The previous receipt's [`hash`](ToolReceipt::hash), linking the chain.
    pub prev_hash: ReceiptHash,
    /// This receipt's SHA-256 chain hash (see module docs) (R5.2).
    pub hash: ReceiptHash,
    /// `ed25519(secret_key, hash)` — the install-key signature over `hash`.
    #[serde(with = "signature_bytes")]
    pub signature: [u8; SIGNATURE_LEN],
}

/// The fields that are bound into a receipt's hash, before chaining/signing.
///
/// This is the canonical input set for [`compute_hash`]; keeping it as its own
/// struct lets the append path and task 4.2's `verify()` recompute a hash from
/// identical data without duplicating the field list.
#[derive(Debug, Clone)]
pub struct ReceiptContent<'a> {
    /// Monotonic sequence number.
    pub seq: u64,
    /// The session this action belongs to (bound into the hash so a stored
    /// receipt cannot be reassigned to a different session undetected).
    pub session_id: SessionId,
    /// Recorded timestamp (UTC).
    pub timestamp: DateTime<Utc>,
    /// The action description.
    pub action: &'a str,
    /// SHA-256 digest of the inputs.
    pub inputs_digest: InputsDigest,
    /// The deciding Model_Provider alias.
    pub deciding_model: &'a str,
    /// The previous receipt's hash.
    pub prev_hash: ReceiptHash,
}

/// Computes a receipt's chain hash:
/// `SHA256(seq || session_id || timestamp || action || inputs_digest || deciding_model || prev_hash)`.
///
/// Field framing is unambiguous: the `seq` and `timestamp` are fixed-width
/// big-endian integers, the `session_id` is its 16 fixed-width UUID bytes, and
/// the two variable-length string fields are each prefixed with their byte
/// length (also big-endian `u64`). This prevents two different field boundaries
/// from hashing to the same bytes.
///
/// Shared by [`append`](crate::Ledger::append) and `verify()`.
#[must_use]
pub fn compute_hash(content: &ReceiptContent<'_>) -> ReceiptHash {
    let mut hasher = Sha256::new();
    // seq: fixed 8 bytes.
    hasher.update(content.seq.to_be_bytes());
    // session_id: fixed 16 UUID bytes (no length prefix needed). Binding it
    // means a stored receipt cannot be reassigned to a different session
    // without `verify()` detecting it (R5.1, R5.5).
    hasher.update(content.session_id.as_uuid().as_bytes());
    // timestamp: fixed 8 bytes (nanoseconds since the Unix epoch).
    hasher.update(timestamp_nanos(content.timestamp).to_be_bytes());
    // action: length-prefixed UTF-8.
    update_length_prefixed(&mut hasher, content.action.as_bytes());
    // inputs_digest: fixed 32 bytes.
    hasher.update(content.inputs_digest.as_slice());
    // deciding_model: length-prefixed UTF-8.
    update_length_prefixed(&mut hasher, content.deciding_model.as_bytes());
    // prev_hash: fixed 32 bytes.
    hasher.update(content.prev_hash.as_slice());
    ReceiptHash(hasher.finalize().into())
}

/// Feeds `bytes` into the hasher prefixed by its big-endian `u64` length, so
/// variable-length fields cannot blur into adjacent fields.
fn update_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

/// Returns a stable integer encoding of a timestamp for hashing.
///
/// Uses nanoseconds since the Unix epoch when representable, otherwise falls
/// back to whole seconds. The same encoding is used on append and verify so the
/// hash is reproducible.
fn timestamp_nanos(ts: DateTime<Utc>) -> i64 {
    ts.timestamp_nanos_opt()
        .unwrap_or_else(|| ts.timestamp().saturating_mul(1_000_000_000))
}

/// serde adapter so the fixed `[u8; 64]` signature round-trips as a byte array
/// (serde does not derive (de)serialize for arrays longer than 32).
mod signature_bytes {
    use super::SIGNATURE_LEN;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes the signature as a `Vec<u8>`.
    pub fn serialize<S: Serializer>(
        sig: &[u8; SIGNATURE_LEN],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        sig.to_vec().serialize(serializer)
    }

    /// Deserializes a `Vec<u8>` back into a fixed-size signature array.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; SIGNATURE_LEN], D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        bytes.try_into().map_err(|v: Vec<u8>| {
            serde::de::Error::invalid_length(v.len(), &"a 64-byte ed25519 signature")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{compute_hash, digest_inputs, InputsDigest, ReceiptContent, ReceiptHash};
    use chrono::{TimeZone, Utc};
    use cyrene_core::SessionId;
    use uuid::Uuid;

    /// A fixed, deterministic session id for hash-content tests.
    fn test_session() -> SessionId {
        SessionId::from_uuid(Uuid::from_u128(1))
    }

    #[test]
    fn inputs_digest_is_deterministic_and_hides_raw_input() {
        let secret = "api_key=super-secret-token";
        let d1 = digest_inputs(secret);
        let d2 = InputsDigest::of(secret);
        assert_eq!(d1, d2, "digest must be deterministic for the same input");
        // The digest bytes must not contain the raw secret bytes verbatim.
        assert_ne!(d1.as_slice(), secret.as_bytes());
    }

    #[test]
    fn different_inputs_produce_different_digests() {
        assert_ne!(digest_inputs("a"), digest_inputs("b"));
    }

    #[test]
    fn hash_is_deterministic_for_identical_content() {
        let ts = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let content = ReceiptContent {
            seq: 1,
            session_id: test_session(),
            timestamp: ts,
            action: "write_file",
            inputs_digest: digest_inputs("path=/tmp/x"),
            deciding_model: "local-ollama",
            prev_hash: ReceiptHash::ZERO,
        };
        assert_eq!(compute_hash(&content), compute_hash(&content));
    }

    #[test]
    fn hash_changes_when_any_field_changes() {
        let ts = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let base = ReceiptContent {
            seq: 1,
            session_id: test_session(),
            timestamp: ts,
            action: "write_file",
            inputs_digest: digest_inputs("a"),
            deciding_model: "local",
            prev_hash: ReceiptHash::ZERO,
        };
        let base_hash = compute_hash(&base);

        let changed_seq = ReceiptContent {
            seq: 2,
            ..base.clone()
        };
        let changed_session = ReceiptContent {
            session_id: SessionId::from_uuid(Uuid::from_u128(2)),
            ..base.clone()
        };
        let changed_action = ReceiptContent {
            action: "delete_file",
            ..base.clone()
        };
        let changed_inputs = ReceiptContent {
            inputs_digest: digest_inputs("b"),
            ..base.clone()
        };
        let changed_model = ReceiptContent {
            deciding_model: "premium",
            ..base.clone()
        };
        let changed_prev = ReceiptContent {
            prev_hash: ReceiptHash([1u8; 32]),
            ..base.clone()
        };

        for other in [
            changed_seq,
            changed_session,
            changed_action,
            changed_inputs,
            changed_model,
            changed_prev,
        ] {
            assert_ne!(base_hash, compute_hash(&other));
        }
    }

    #[test]
    fn length_prefix_prevents_field_boundary_collision() {
        let ts = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        // ("ab","c") vs ("a","bc") must not hash the same despite concatenating
        // to the same bytes, thanks to length prefixing.
        let left = ReceiptContent {
            seq: 1,
            session_id: test_session(),
            timestamp: ts,
            action: "ab",
            inputs_digest: InputsDigest([0u8; 32]),
            deciding_model: "c",
            prev_hash: ReceiptHash::ZERO,
        };
        let right = ReceiptContent {
            action: "a",
            deciding_model: "bc",
            ..left.clone()
        };
        assert_ne!(compute_hash(&left), compute_hash(&right));
    }
}
