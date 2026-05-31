//! `cyrene-ledger`: the signed, append-only, hash-chained Receipt_Ledger for Cyrene.
//!
//! Every action and decision the Agent_Loop takes is recorded here as a signed,
//! immutable [`ToolReceipt`] (R5.1). The ledger is a SQLite-backed hash chain:
//! each receipt's `hash` is
//! `SHA256(seq || session_id || timestamp || action || inputs_digest || deciding_model || prev_hash)`
//! and `prev_hash` links to the previous receipt, so any retroactive edit is
//! detectable (R5.2). Each `hash` is then signed with an ed25519 install
//! keypair (`ed25519(secret_key, hash)`), generated at install and stored
//! `0600` (R5.2). Receipts are stored append-only, ordered by a monotonic
//! `seq` (R5.4).
//!
//! Inputs are stored only as a SHA-256 [`InputsDigest`] — never as the raw
//! bytes — so secret-bearing tool arguments are never persisted verbatim. Use
//! [`digest_inputs`] at the call site to produce that digest.
//!
//! Task 4.1 implements the model, schema, install keypair, and
//! [`Ledger::append`]. Task 4.2 adds [`Ledger::verify`] — which walks the chain
//! in `seq` order, recomputes each hash, checks the chain links and each
//! signature, and localizes the first divergence (R5.3) — and
//! [`Ledger::receipts_for_session`], the per-session chronological query
//! (R5.5). Both reuse the chain/sign primitives ([`compute_hash`],
//! [`InstallKey::verify_signature`]) and the `seq`-ordered read path rather than
//! reworking the append-only write path.

mod error;
mod keypair;
mod ledger;
mod receipt;
mod verify;

pub use error::LedgerError;
pub use keypair::InstallKey;
pub use ledger::Ledger;
pub use receipt::{
    compute_hash, digest_inputs, InputsDigest, ReceiptContent, ReceiptHash, ToolReceipt, HASH_LEN,
    SIGNATURE_LEN,
};
pub use verify::{DivergenceKind, LedgerVerification};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-ledger"
}

#[cfg(test)]
mod tests {
    use super::{digest_inputs, InstallKey, Ledger, ReceiptHash};
    use cyrene_core::SessionId;

    /// An in-memory ledger with a freshly generated key, for fast unit tests.
    fn test_ledger() -> Ledger {
        // Persist a key to a temp file so generation + load both get exercised.
        let dir = tempfile::tempdir().unwrap();
        let key = InstallKey::load_or_generate(InstallKey::default_path_in(dir.path())).unwrap();
        Ledger::open_in_memory(key).unwrap()
    }

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!super::subsystem().is_empty());
    }

    #[test]
    fn first_append_starts_at_seq_zero_with_genesis_prev_hash() {
        let ledger = test_ledger();
        let session = SessionId::new();

        let receipt = ledger
            .append(session, "plan", digest_inputs("hello"), "local-ollama")
            .unwrap();

        assert_eq!(receipt.seq, 0);
        assert_eq!(receipt.prev_hash, ReceiptHash::ZERO);
        assert_eq!(receipt.session_id, session);
        assert_eq!(receipt.action, "plan");
    }

    #[test]
    fn append_two_receipts_increments_seq_and_chains_prev_hash() {
        let ledger = test_ledger();
        let session = SessionId::new();

        let first = ledger
            .append(session, "step-1", digest_inputs("in-1"), "local")
            .unwrap();
        let second = ledger
            .append(session, "step-2", digest_inputs("in-2"), "premium")
            .unwrap();

        // seq strictly increases (R5.4)...
        assert_eq!(first.seq, 0);
        assert_eq!(second.seq, 1);
        // ...and the chain links: receipt[n].prev_hash == receipt[n-1].hash.
        assert_eq!(second.prev_hash, first.hash);
        assert_ne!(first.hash, second.hash);
    }

    #[test]
    fn stored_receipts_round_trip_in_seq_order() {
        let ledger = test_ledger();
        let session = SessionId::new();

        let a = ledger
            .append(session, "a", digest_inputs("a"), "local")
            .unwrap();
        let b = ledger
            .append(session, "b", digest_inputs("b"), "local")
            .unwrap();

        let stored = ledger.all_receipts().unwrap();
        assert_eq!(stored, vec![a, b]);
        assert_eq!(ledger.len().unwrap(), 2);
        assert!(!ledger.is_empty().unwrap());
    }

    #[test]
    fn signature_verifies_against_the_install_public_key() {
        let ledger = test_ledger();
        let session = SessionId::new();

        let receipt = ledger
            .append(session, "act", digest_inputs("x"), "local")
            .unwrap();

        let verifying_key = ledger.install_key().verifying_key();
        assert!(InstallKey::verify_signature(
            &verifying_key,
            &receipt.hash,
            &receipt.signature
        ));
    }

    #[test]
    fn raw_inputs_are_not_stored_only_their_digest() {
        let ledger = test_ledger();
        let session = SessionId::new();
        let secret = "password=hunter2";

        let receipt = ledger
            .append(session, "login", digest_inputs(secret), "local")
            .unwrap();

        // The stored digest matches the digest of the secret, and is not the
        // raw secret bytes.
        assert_eq!(receipt.inputs_digest, digest_inputs(secret));
        assert_ne!(receipt.inputs_digest.as_slice(), secret.as_bytes());
    }

    #[test]
    fn install_key_persists_and_reloads_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = InstallKey::default_path_in(dir.path());

        let generated = InstallKey::load_or_generate(&path).unwrap();
        let reloaded = InstallKey::load_or_generate(&path).unwrap();

        // Same secret seed → same derived public key.
        assert_eq!(
            generated.verifying_key().to_bytes(),
            reloaded.verifying_key().to_bytes()
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_key_file_is_created_0600_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = InstallKey::default_path_in(dir.path());
        let _ = InstallKey::load_or_generate(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        // Only the owner-read/write bits may be set.
        assert_eq!(mode & 0o777, 0o600, "key file must be 0600");
    }

    #[test]
    fn receipt_serde_round_trip_preserves_signature() {
        let ledger = test_ledger();
        let session = SessionId::new();
        let receipt = ledger
            .append(session, "act", digest_inputs("x"), "local")
            .unwrap();

        let json = serde_json::to_string(&receipt).unwrap();
        let back: super::ToolReceipt = serde_json::from_str(&json).unwrap();
        assert_eq!(receipt, back);
    }
}
