//! Property-based tests for the Shadow Executor and Sandbox containment.
//!
//! **Validates: Requirements 3.1, 3.2, 22.3**
//!
//! Property 6: No real side effect before approval — for any plan containing an
//! `Irreversible_Action`, no real resource is mutated until shadow execution has
//! produced a summary and the user has approved.
//!
//! Property 7: Sandbox containment — during shadow execution and all sandboxed
//! steps, no write occurs outside the sandbox/workspace boundary.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use cyrene_core::{Plan, Risk, SessionId, Step, StepKind, ToolCall};
use cyrene_safety::{Sandbox, SandboxBackend, SandboxError, ShadowExecutor};
use proptest::prelude::*;
use serde_json::json;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Create a test workspace with a few files for snapshotting.
fn create_test_workspace() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
    fs::write(root.join("README.md"), "# Test Project").unwrap();
    dir
}

/// Snapshot all file hashes under a directory, recursively.
fn snapshot_hashes(root: &Path) -> HashMap<PathBuf, Vec<u8>> {
    let mut map = HashMap::new();
    snapshot_hashes_recursive(root, root, &mut map);
    map
}

fn snapshot_hashes_recursive(base: &Path, current: &Path, map: &mut HashMap<PathBuf, Vec<u8>>) {
    if let Ok(entries) = fs::read_dir(current) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                snapshot_hashes_recursive(base, &path, map);
            } else if path.is_file() {
                let relative = path.strip_prefix(base).unwrap().to_path_buf();
                let content = fs::read(&path).unwrap_or_default();
                // Use a simple hash for comparison (not cryptographic, but sufficient
                // for detecting changes in tests).
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                content.hash(&mut hasher);
                map.insert(relative, hasher.finish().to_le_bytes().to_vec());
            }
        }
    }
}

// ─── Proptest Strategies ─────────────────────────────────────────────────────

/// Strategy for generating a valid file path (workspace-relative).
fn arb_workspace_path() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("src/main.rs".to_string()),
        Just("src/lib.rs".to_string()),
        Just("Cargo.toml".to_string()),
        Just("README.md".to_string()),
        Just("new_file.txt".to_string()),
        Just("src/utils.rs".to_string()),
        Just("docs/guide.md".to_string()),
        Just("tests/test1.rs".to_string()),
    ]
}

/// Strategy for generating file content.
fn arb_content() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _\\-\\.\\n]{0,200}"
}

/// Strategy for generating a StepKind.
fn arb_step_kind() -> impl Strategy<Value = StepKind> {
    prop_oneof![
        Just(StepKind::FileEdit),
        Just(StepKind::ExternalAction),
        Just(StepKind::ModelQuery),
        Just(StepKind::CommandExec),
        Just(StepKind::ToolCall),
    ]
}

/// Strategy for generating a Risk level.
fn arb_risk() -> impl Strategy<Value = Risk> {
    prop_oneof![Just(Risk::Low), Just(Risk::Medium), Just(Risk::High),]
}

/// Strategy for generating a step with a tool call appropriate for its kind.
fn arb_step(seq: u64, force_irreversible: bool) -> impl Strategy<Value = Step> {
    (
        arb_step_kind(),
        arb_risk(),
        arb_workspace_path(),
        arb_content(),
        any::<bool>(),
    )
        .prop_map(move |(kind, risk, path, content, irreversible_flag)| {
            let tool = match kind {
                StepKind::FileEdit => Some(ToolCall::new(
                    "fs.write",
                    json!({ "path": path, "content": content }),
                )),
                StepKind::ExternalAction => Some(ToolCall::new(
                    "http.post",
                    json!({ "url": "https://api.example.com/action", "body": content }),
                )),
                StepKind::ModelQuery => {
                    Some(ToolCall::new("model.query", json!({ "prompt": content })))
                }
                StepKind::CommandExec => {
                    Some(ToolCall::new("shell.run", json!({ "cmd": "echo test" })))
                }
                StepKind::ToolCall => Some(ToolCall::new("custom.tool", json!({ "arg": content }))),
            };

            let mut step = Step::new(seq, kind, risk);
            if let Some(t) = tool {
                step = step.with_tool(t);
            }
            if force_irreversible || irreversible_flag {
                step = step.irreversible();
            }
            step
        })
}

/// Strategy for generating a plan with 1..=8 steps where at least one is irreversible.
fn arb_plan_with_irreversible() -> impl Strategy<Value = Plan> {
    // Generate 1..=7 arbitrary steps, then insert one forced-irreversible step.
    (1..=7usize)
        .prop_flat_map(|extra_count| {
            let extra_steps: Vec<_> = (0..extra_count)
                .map(|i| arb_step(i as u64, false))
                .collect();
            (Just(extra_count), extra_steps.prop_map(|v| v))
        })
        .prop_map(|(extra_count, mut steps)| {
            // Add one guaranteed irreversible step at the end.
            let irrev_seq = extra_count as u64;
            let irrev_step = Step::new(irrev_seq, StepKind::ExternalAction, Risk::High)
                .with_tool(ToolCall::new(
                    "http.post",
                    json!({ "url": "https://api.example.com/deploy", "body": "{}" }),
                ))
                .irreversible();
            steps.push(irrev_step);

            // Re-number sequences.
            for (i, step) in steps.iter_mut().enumerate() {
                step.seq = i as u64;
            }

            Plan::new(SessionId::new(), steps)
        })
}

/// Strategy for paths that attempt to escape the workspace boundary.
fn arb_escape_path() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("/etc/passwd".to_string()),
        Just("/tmp/escape_test".to_string()),
        Just("../escape_file.txt".to_string()),
        Just("../../etc/shadow".to_string()),
        Just("/var/log/syslog".to_string()),
        Just("../../../root/.ssh/id_rsa".to_string()),
    ]
}

/// Strategy for generating a plan with file edit steps, some targeting paths
/// inside the workspace and some targeting paths outside.
fn arb_plan_with_escape_attempts() -> impl Strategy<Value = (Plan, Vec<String>)> {
    // Generate 1..=4 safe file edits and 1..=4 escape attempts.
    (
        prop::collection::vec((arb_workspace_path(), arb_content()), 1..=4),
        prop::collection::vec((arb_escape_path(), arb_content()), 1..=4),
    )
        .prop_map(|(safe_edits, escape_edits)| {
            let mut steps = Vec::new();
            let mut escape_paths = Vec::new();
            let mut seq = 0u64;

            // Add safe file edits.
            for (path, content) in &safe_edits {
                steps.push(
                    Step::new(seq, StepKind::FileEdit, Risk::Low).with_tool(ToolCall::new(
                        "fs.write",
                        json!({ "path": path, "content": content }),
                    )),
                );
                seq += 1;
            }

            // Add escape attempts.
            for (path, content) in &escape_edits {
                escape_paths.push(path.clone());
                steps.push(
                    Step::new(seq, StepKind::FileEdit, Risk::Low).with_tool(ToolCall::new(
                        "fs.write",
                        json!({ "path": path, "content": content }),
                    )),
                );
                seq += 1;
            }

            let plan = Plan::new(SessionId::new(), steps);
            (plan, escape_paths)
        })
}

// ─── Property Tests ──────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// **Property 6 (R3.1): No real side effect before approval.**
    ///
    /// For any plan containing at least one irreversible step, after shadow
    /// execution:
    /// (a) the original workspace directory is UNCHANGED (snapshot file hashes
    ///     before and after, compare),
    /// (b) the summary is produced (not an error), and
    /// (c) all irreversible steps appear in `intercepted_actions`.
    ///
    /// **Validates: Requirements 3.1**
    #[test]
    fn prop_no_real_side_effect_before_approval(plan in arb_plan_with_irreversible()) {
        // Precondition: plan has at least one irreversible step.
        prop_assert!(plan.has_irreversible_action());

        // Create a workspace and snapshot it.
        let workspace = create_test_workspace();
        let workspace_snapshot_before = snapshot_hashes(workspace.path());

        // Create sandbox and execute.
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();
        let executor = ShadowExecutor::new(&sandbox);
        let result = executor.execute(&plan);

        // (b) The summary is produced (not a catastrophic error).
        prop_assert!(result.is_ok(), "Shadow execution returned error: {:?}", result.err());
        let summary = result.unwrap();

        // (a) The original workspace is UNCHANGED.
        let workspace_snapshot_after = snapshot_hashes(workspace.path());
        prop_assert_eq!(
            workspace_snapshot_before,
            workspace_snapshot_after,
            "Workspace was mutated during shadow execution!"
        );

        // (c) All irreversible steps appear in intercepted_actions.
        let irreversible_seqs: Vec<u64> = plan
            .steps
            .iter()
            .filter(|s| s.irreversible)
            .map(|s| s.seq)
            .collect();

        let intercepted_seqs: Vec<u64> = summary
            .intercepted_actions
            .iter()
            .map(|a| a.step_seq)
            .collect();

        for seq in &irreversible_seqs {
            prop_assert!(
                intercepted_seqs.contains(seq),
                "Irreversible step seq={} was NOT intercepted. Intercepted: {:?}",
                seq,
                intercepted_seqs
            );
        }
    }

    /// **Property 7 (R3.2, R22.3): Sandbox containment.**
    ///
    /// For any plan with arbitrary file edit steps (some targeting paths inside
    /// the workspace, some targeting paths OUTSIDE like "/etc/passwd" or
    /// "../escape"), shadow execution either:
    /// (a) succeeds with all file changes confined to the sandbox root, or
    /// (b) returns a DeniedAccess error for the out-of-bounds path.
    ///
    /// In either case, no write occurs outside the sandbox boundary. Verified
    /// by checking that a sentinel file placed outside the sandbox before
    /// execution is unchanged after.
    ///
    /// **Validates: Requirements 3.2, 22.3**
    #[test]
    fn prop_sandbox_containment_no_write_escapes(
        (plan, escape_paths) in arb_plan_with_escape_attempts()
    ) {
        // Create workspace and sandbox.
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // Place a sentinel file OUTSIDE the sandbox to detect any escape.
        let sentinel_dir = tempfile::TempDir::new().unwrap();
        let sentinel_path = sentinel_dir.path().join("sentinel.txt");
        let sentinel_content = "SENTINEL_UNCHANGED";
        fs::write(&sentinel_path, sentinel_content).unwrap();

        // Snapshot the workspace (original) before execution.
        let workspace_snapshot_before = snapshot_hashes(workspace.path());

        let executor = ShadowExecutor::new(&sandbox);
        let result = executor.execute(&plan);

        // The sentinel file must be unchanged regardless of outcome.
        let sentinel_after = fs::read_to_string(&sentinel_path).unwrap();
        prop_assert_eq!(
            sentinel_after.as_str(),
            sentinel_content,
            "Sentinel file outside sandbox was modified!"
        );

        // The original workspace must be unchanged.
        let workspace_snapshot_after = snapshot_hashes(workspace.path());
        prop_assert_eq!(
            workspace_snapshot_before,
            workspace_snapshot_after,
            "Original workspace was mutated during shadow execution!"
        );

        match result {
            Ok(summary) => {
                // If execution succeeded, verify all file changes are within
                // the sandbox root. The sandbox translates workspace-relative
                // paths, so any file_change path should be workspace-relative
                // (not an absolute escape path).
                for change in &summary.file_changes {
                    let change_path = &change.path;
                    // The path in file_changes is workspace-relative as passed
                    // to the tool. It must NOT be an absolute path outside the
                    // workspace.
                    prop_assert!(
                        !change_path.starts_with("/etc")
                            && !change_path.starts_with("/tmp")
                            && !change_path.starts_with("/var")
                            && !change_path.starts_with("/root"),
                        "File change recorded for path outside workspace: {:?}",
                        change_path
                    );
                }

                // Verify no file was created outside the sandbox root.
                for escape_path in &escape_paths {
                    let abs_escape = Path::new(escape_path);
                    if abs_escape.is_absolute() {
                        prop_assert!(
                            !abs_escape.exists()
                                || (abs_escape == Path::new("/etc/passwd")
                                    || abs_escape == Path::new("/var/log/syslog")),
                            "Escape path was created: {:?}",
                            abs_escape
                        );
                    }
                }
            }
            Err(err) => {
                // If execution failed, it must be a DeniedAccess error for an
                // out-of-bounds path (or an I/O error from path translation).
                match &err {
                    SandboxError::DeniedAccess { path, .. } => {
                        // The denied path should be one of our escape attempts
                        // or a path that resolved outside the workspace.
                        let path_str = path.to_string_lossy().to_string();
                        prop_assert!(
                            escape_paths.iter().any(|ep| path_str.contains(ep)
                                || ep.contains(&path_str))
                                || path_str.contains("outside"),
                            "DeniedAccess for unexpected path: {:?}",
                            path
                        );
                    }
                    SandboxError::Io { context, .. } => {
                        // I/O errors from path translation are acceptable
                        // (e.g., "path is not within workspace root").
                        prop_assert!(
                            context.contains("step") || context.contains("path"),
                            "Unexpected I/O error context: {}",
                            context
                        );
                    }
                    other => {
                        prop_assert!(
                            false,
                            "Unexpected error type during sandbox containment test: {:?}",
                            other
                        );
                    }
                }
            }
        }
    }
}
