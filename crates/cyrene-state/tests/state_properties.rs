//! Property-based tests for the State_Tree correctness invariants.
//!
//! - **Property 3 (R4.2):** `checkout(checkpoint(state)) == state` for memory,
//!   file bytes, and session variables.
//! - **Property 4 (R4.5):** Advancing from a restored checkpoint never deletes
//!   or overwrites superseded checkpoints; they remain reachable on a sibling
//!   branch.

use proptest::prelude::*;

use cyrene_core::{BranchId, SessionId};
use cyrene_state::StateStore;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Generates a short alphanumeric path component (1..=8 chars).
fn arb_path_segment() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,7}"
}

/// Generates a relative file path like "dir/file.rs" (1–2 segments).
fn arb_file_path() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_path_segment(),
        (arb_path_segment(), arb_path_segment()).prop_map(|(a, b)| format!("{a}/{b}")),
    ]
}

/// Generates a set of files: Vec<(path, content)> with 0..8 files, each 0..128
/// bytes, and unique paths.
fn arb_files() -> impl Strategy<Value = Vec<(String, Vec<u8>)>> {
    prop::collection::vec(
        (arb_file_path(), prop::collection::vec(any::<u8>(), 0..=128)),
        0..=8,
    )
    .prop_map(|files| {
        // Deduplicate paths (keep last occurrence).
        let mut seen = std::collections::BTreeMap::new();
        for (path, content) in files {
            seen.insert(path, content);
        }
        seen.into_iter().collect::<Vec<_>>()
    })
}

// ---------------------------------------------------------------------------
// Property 3: Checkpoint round-trip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// **Validates: Requirements 4.2**
    ///
    /// For arbitrary mem_blob, vars_blob, and a set of files, after
    /// `checkpoint(...)` then `checkout(id)`, the returned Checkpoint's
    /// `mem_blob == original mem_blob`, `vars_blob == original vars_blob`, and
    /// for every file in the manifest, `read_blob(hash) == original file bytes`.
    #[test]
    fn prop3_checkout_checkpoint_round_trip(
        mem_blob in prop::collection::vec(any::<u8>(), 0..=256),
        vars_blob in prop::collection::vec(any::<u8>(), 0..=256),
        files in arb_files(),
    ) {
        let store = StateStore::open_in_memory().unwrap();
        let session = SessionId::new();
        let branch = BranchId::new();

        // Build the files slice in the format checkpoint() expects.
        let files_ref: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_slice()))
            .collect();

        let id = store
            .checkpoint(
                session,
                0,
                None,
                branch,
                "prop3 checkpoint",
                &mem_blob,
                &vars_blob,
                &files_ref,
            )
            .unwrap();

        // Checkout the checkpoint and verify round-trip equality.
        let cp = store.checkout(id).unwrap();

        // Memory blob round-trips.
        prop_assert_eq!(&cp.mem_blob, &mem_blob, "mem_blob mismatch");

        // Vars blob round-trips.
        prop_assert_eq!(&cp.vars_blob, &vars_blob, "vars_blob mismatch");

        // Every file in the manifest round-trips via read_blob.
        prop_assert_eq!(
            cp.file_manifest.len(),
            files.len(),
            "file_manifest length mismatch"
        );

        for (path, original_content) in &files {
            let hash = cp
                .file_manifest
                .get(path)
                .unwrap_or_else(|| panic!("path {path} missing from manifest"));
            let stored = store
                .read_blob(hash)
                .unwrap()
                .unwrap_or_else(|| panic!("blob missing for path {path}"));
            prop_assert_eq!(
                &stored,
                original_content,
                "file content mismatch for path {}",
                path
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Property 4: Branch preservation
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// **Validates: Requirements 4.5**
    ///
    /// For an arbitrary chain of N checkpoints (1..=16) on a branch, after
    /// checking out an arbitrary earlier checkpoint and forking
    /// (`fork_branch`), then creating M new checkpoints on the new branch: ALL
    /// original N checkpoints still exist (`get()` returns `Some` for each),
    /// their data is unchanged, and the new branch's checkpoints coexist
    /// without overwriting. No checkpoint id that existed before the fork
    /// returns `None` after the fork + advance.
    #[test]
    fn prop4_fork_preserves_all_checkpoints(
        n in 1..=16usize,
        m in 1..=8usize,
        // Which of the N checkpoints to restore from (0-indexed).
        restore_idx_raw in 0..16usize,
    ) {
        let store = StateStore::open_in_memory().unwrap();
        let session = SessionId::new();
        let branch = BranchId::new();

        // Clamp restore_idx to valid range [0, n-1].
        let restore_idx = restore_idx_raw % n;

        // --- Phase 1: Create N checkpoints on the original branch. ---
        let mut original_ids = Vec::with_capacity(n);
        let mut parent = None;

        for seq in 0..n {
            let mem = format!("mem-{seq}").into_bytes();
            let vars = format!("vars-{seq}").into_bytes();
            let id = store
                .checkpoint(
                    session,
                    seq as u64,
                    parent,
                    branch,
                    format!("step {seq}"),
                    &mem,
                    &vars,
                    &[],
                )
                .unwrap();
            original_ids.push(id);
            parent = Some(id);
        }

        // Snapshot the original checkpoint data for later comparison.
        let original_snapshots: Vec<_> = original_ids
            .iter()
            .map(|&id| store.get(id).unwrap().unwrap())
            .collect();

        // --- Phase 2: Checkout an earlier checkpoint and fork. ---
        let restore_id = original_ids[restore_idx];
        let _restored = store.checkout(restore_id).unwrap();
        let new_branch = store.fork_branch(restore_id).unwrap();

        // --- Phase 3: Create M new checkpoints on the forked branch. ---
        let mut new_ids = Vec::with_capacity(m);
        let mut fork_parent = Some(restore_id);

        for i in 0..m {
            let seq = (n + i) as u64;
            let mem = format!("fork-mem-{i}").into_bytes();
            let vars = format!("fork-vars-{i}").into_bytes();
            let id = store
                .checkpoint(
                    session,
                    seq,
                    fork_parent,
                    new_branch,
                    format!("fork step {i}"),
                    &mem,
                    &vars,
                    &[],
                )
                .unwrap();
            new_ids.push(id);
            fork_parent = Some(id);
        }

        // --- Assertions ---

        // All original checkpoints still exist and are unchanged.
        for (i, &id) in original_ids.iter().enumerate() {
            let cp = store.get(id).unwrap();
            prop_assert!(
                cp.is_some(),
                "original checkpoint {} (id={}) was lost after fork+advance",
                i,
                id
            );
            let cp = cp.unwrap();
            prop_assert_eq!(
                &cp, &original_snapshots[i],
                "original checkpoint {} data changed after fork+advance",
                i
            );
        }

        // All new checkpoints exist on the forked branch.
        for (i, &id) in new_ids.iter().enumerate() {
            let cp = store.get(id).unwrap();
            prop_assert!(
                cp.is_some(),
                "new checkpoint {} (id={}) missing after creation",
                i,
                id
            );
            let cp = cp.unwrap();
            prop_assert_eq!(
                cp.branch_id, new_branch,
                "new checkpoint {} not on the forked branch",
                i
            );
        }

        // The new branch is distinct from the original.
        prop_assert_ne!(new_branch, branch, "forked branch should differ from original");
    }
}
