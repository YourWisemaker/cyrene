//! Shadow Executor: runs a full plan in the sandbox, intercepts
//! irreversible/external calls (records, never performs), and produces a
//! [`ProjectedOutcomeSummary`] of file changes and would-be external actions.
//!
//! On a failed step, the executor reports the failure and withholds real
//! execution (R3.5).

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use cyrene_core::{Plan, Step, StepKind};

use crate::sandbox::error::SandboxError;
use crate::sandbox::Sandbox;

/// A record of a single file change observed during shadow execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// The workspace-relative path of the affected file.
    pub path: PathBuf,
    /// The kind of change observed.
    pub kind: FileChangeKind,
    /// Size in bytes before the change (0 for created files).
    pub before_size: u64,
    /// Size in bytes after the change (0 for deleted files).
    pub after_size: u64,
}

/// The kind of file change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    /// A new file was created.
    Created,
    /// An existing file was modified.
    Modified,
    /// A file was deleted.
    Deleted,
}

impl fmt::Display for FileChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Modified => write!(f, "modified"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

/// A record of an intercepted external/irreversible action that was NOT
/// performed but would have been during real execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterceptedAction {
    /// The step sequence number that would have performed this action.
    pub step_seq: u64,
    /// A human-readable description of the action.
    pub description: String,
    /// The arguments that would have been passed (serialized as a string).
    pub args: String,
}

/// Information about a step that failed during shadow execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedStep {
    /// The sequence number of the step that failed.
    pub seq: u64,
    /// A human-readable description of the error.
    pub error: String,
}

/// The complete result of shadow-executing a plan in the sandbox.
///
/// Lists file changes (creates/modifies/deletes), intercepted external actions
/// that would have been performed, and an optional failed step if execution
/// was halted early.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedOutcomeSummary {
    /// File changes observed during shadow execution.
    pub file_changes: Vec<FileChange>,
    /// External/irreversible actions that were intercepted (recorded but never
    /// performed).
    pub intercepted_actions: Vec<InterceptedAction>,
    /// If a step failed during shadow execution, this records which step and
    /// why. When present, real execution should be withheld (R3.5).
    pub failed_step: Option<FailedStep>,
}

impl ProjectedOutcomeSummary {
    /// Returns `true` if shadow execution completed without any step failing.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.failed_step.is_none()
    }

    /// Returns `true` if any external/irreversible actions were intercepted.
    #[must_use]
    pub fn has_intercepted_actions(&self) -> bool {
        !self.intercepted_actions.is_empty()
    }
}

impl fmt::Display for ProjectedOutcomeSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "═══ Projected Outcome Summary ═══")?;
        writeln!(f)?;

        // File changes section.
        if self.file_changes.is_empty() {
            writeln!(f, "File changes: none")?;
        } else {
            writeln!(f, "File changes ({}):", self.file_changes.len())?;
            for change in &self.file_changes {
                match change.kind {
                    FileChangeKind::Created => {
                        writeln!(
                            f,
                            "  + {} (created, {} bytes)",
                            change.path.display(),
                            change.after_size
                        )?;
                    }
                    FileChangeKind::Modified => {
                        writeln!(
                            f,
                            "  ~ {} (modified, {} → {} bytes)",
                            change.path.display(),
                            change.before_size,
                            change.after_size
                        )?;
                    }
                    FileChangeKind::Deleted => {
                        writeln!(
                            f,
                            "  - {} (deleted, was {} bytes)",
                            change.path.display(),
                            change.before_size
                        )?;
                    }
                }
            }
        }
        writeln!(f)?;

        // Intercepted actions section.
        if self.intercepted_actions.is_empty() {
            writeln!(f, "Intercepted external actions: none")?;
        } else {
            writeln!(
                f,
                "Intercepted external actions ({}):",
                self.intercepted_actions.len()
            )?;
            for action in &self.intercepted_actions {
                writeln!(
                    f,
                    "  [step {}] {} (args: {})",
                    action.step_seq, action.description, action.args
                )?;
            }
        }
        writeln!(f)?;

        // Failed step section.
        if let Some(failed) = &self.failed_step {
            writeln!(f, "⚠ FAILED at step {}: {}", failed.seq, failed.error)?;
            writeln!(f, "  Real execution withheld pending user review.")?;
        } else {
            writeln!(f, "✓ All steps completed successfully in shadow.")?;
        }

        Ok(())
    }
}

/// Configuration for shadow execution behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShadowExecutionConfig {
    /// When `true`, shadow execution runs for every plan regardless of whether
    /// it contains irreversible actions (R3.6).
    pub mandatory: bool,
}

impl ShadowExecutionConfig {
    /// Create a config where shadow execution is mandatory for all plans.
    #[must_use]
    pub fn mandatory() -> Self {
        Self { mandatory: true }
    }

    /// Returns whether shadow execution should run for the given plan.
    ///
    /// Shadow execution is required when:
    /// - The config is set to mandatory (R3.6), OR
    /// - The plan contains at least one irreversible action (R3.1).
    #[must_use]
    pub fn should_shadow_execute(&self, plan: &Plan) -> bool {
        self.mandatory || plan.has_irreversible_action()
    }
}

/// The Shadow Executor: runs a full plan in the sandbox, intercepts
/// irreversible/external calls, and produces a [`ProjectedOutcomeSummary`].
///
/// External/irreversible actions are NEVER performed — only recorded.
pub struct ShadowExecutor<'a> {
    sandbox: &'a Sandbox,
}

impl<'a> ShadowExecutor<'a> {
    /// Create a new shadow executor bound to the given sandbox.
    #[must_use]
    pub fn new(sandbox: &'a Sandbox) -> Self {
        Self { sandbox }
    }

    /// Execute the plan in the sandbox and produce a projected outcome summary.
    ///
    /// - File operations are executed against the sandbox root.
    /// - External/irreversible actions are intercepted and recorded.
    /// - Other steps (ModelQuery, CommandExec, ToolCall) are recorded as
    ///   "would execute" entries.
    /// - If any step fails, execution stops and the failure is recorded.
    ///
    /// # Errors
    ///
    /// Returns `SandboxError` only for catastrophic sandbox failures (e.g.
    /// the sandbox root is inaccessible). Step-level failures are captured
    /// in the summary's `failed_step` field.
    pub fn execute(&self, plan: &Plan) -> Result<ProjectedOutcomeSummary, SandboxError> {
        let mut file_changes = Vec::new();
        let mut intercepted_actions = Vec::new();
        let mut failed_step = None;

        for step in &plan.steps {
            let result = self.execute_step(step, &mut file_changes, &mut intercepted_actions);
            if let Err(err) = result {
                failed_step = Some(FailedStep {
                    seq: step.seq,
                    error: err.to_string(),
                });
                // Stop execution on failure (R3.5).
                break;
            }
        }

        Ok(ProjectedOutcomeSummary {
            file_changes,
            intercepted_actions,
            failed_step,
        })
    }

    /// Execute a single step, dispatching by kind and irreversibility.
    fn execute_step(
        &self,
        step: &Step,
        file_changes: &mut Vec<FileChange>,
        intercepted_actions: &mut Vec<InterceptedAction>,
    ) -> Result<(), SandboxError> {
        // Any step marked irreversible is intercepted regardless of kind.
        if step.irreversible {
            intercepted_actions.push(InterceptedAction {
                step_seq: step.seq,
                description: format!("irreversible {:?} action", step.kind),
                args: step
                    .tool
                    .as_ref()
                    .map(|t| format!("{}({})", t.name, t.args))
                    .unwrap_or_else(|| "no tool".to_string()),
            });
            return Ok(());
        }

        match step.kind {
            StepKind::FileEdit => self.execute_file_edit(step, file_changes),
            StepKind::ExternalAction => {
                // External actions are always intercepted, never performed.
                intercepted_actions.push(InterceptedAction {
                    step_seq: step.seq,
                    description: "external action".to_string(),
                    args: step
                        .tool
                        .as_ref()
                        .map(|t| format!("{}({})", t.name, t.args))
                        .unwrap_or_else(|| "no tool".to_string()),
                });
                Ok(())
            }
            StepKind::ModelQuery | StepKind::CommandExec | StepKind::ToolCall => {
                // Record as "would execute" — these are simulated.
                intercepted_actions.push(InterceptedAction {
                    step_seq: step.seq,
                    description: format!("would execute {:?}", step.kind),
                    args: step
                        .tool
                        .as_ref()
                        .map(|t| format!("{}({})", t.name, t.args))
                        .unwrap_or_else(|| "no tool".to_string()),
                });
                Ok(())
            }
        }
    }

    /// Execute a file edit step against the sandbox.
    ///
    /// Translates the workspace path to the sandbox, performs the edit, and
    /// records the file change.
    fn execute_file_edit(
        &self,
        step: &Step,
        file_changes: &mut Vec<FileChange>,
    ) -> Result<(), SandboxError> {
        let tool = step.tool.as_ref().ok_or_else(|| SandboxError::Io {
            context: format!("step {} is a FileEdit but has no tool call", step.seq),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "FileEdit step missing tool call",
            ),
        })?;

        // Extract the path from the tool args.
        let path_str = tool
            .args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SandboxError::Io {
                context: format!(
                    "step {} FileEdit tool `{}` missing `path` argument",
                    step.seq, tool.name
                ),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "missing path argument",
                ),
            })?;

        let workspace_path = Path::new(path_str);
        let sandbox_path = self.sandbox.translate_path(workspace_path)?;

        // Check boundary enforcement.
        if !self.sandbox.is_path_allowed(&sandbox_path) {
            return Err(self.sandbox.deny_write(&sandbox_path));
        }

        // Determine before-state.
        let before_size = fs::metadata(&sandbox_path).map(|m| m.len()).unwrap_or(0);
        let existed_before = sandbox_path.exists();

        // Extract content from tool args and write it.
        let content = tool
            .args
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Ensure parent directory exists.
        if let Some(parent) = sandbox_path.parent() {
            fs::create_dir_all(parent).map_err(|e| SandboxError::Io {
                context: format!("creating parent dirs for `{}`", sandbox_path.display()),
                source: e,
            })?;
        }

        // Check if this is a delete operation.
        let is_delete = tool
            .args
            .get("delete")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if is_delete {
            if existed_before {
                fs::remove_file(&sandbox_path).map_err(|e| SandboxError::Io {
                    context: format!("deleting `{}`", sandbox_path.display()),
                    source: e,
                })?;
                file_changes.push(FileChange {
                    path: workspace_path.to_path_buf(),
                    kind: FileChangeKind::Deleted,
                    before_size,
                    after_size: 0,
                });
            }
        } else {
            fs::write(&sandbox_path, content).map_err(|e| SandboxError::Io {
                context: format!("writing `{}`", sandbox_path.display()),
                source: e,
            })?;

            let after_size = fs::metadata(&sandbox_path).map(|m| m.len()).unwrap_or(0);

            let kind = if existed_before {
                FileChangeKind::Modified
            } else {
                FileChangeKind::Created
            };

            file_changes.push(FileChange {
                path: workspace_path.to_path_buf(),
                kind,
                before_size,
                after_size,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrene_core::Risk;
    use cyrene_core::SessionId;
    use cyrene_core::{Plan, Step, StepKind, ToolCall};
    use serde_json::json;

    use crate::sandbox::{Sandbox, SandboxBackend};

    /// Helper: create a small workspace with a few files for testing.
    fn create_test_workspace() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        dir
    }

    #[test]
    fn safe_file_edits_produce_file_changes_no_intercepted() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();
        let executor = ShadowExecutor::new(&sandbox);

        let plan = Plan::new(
            SessionId::new(),
            vec![
                Step::new(0, StepKind::FileEdit, Risk::Low).with_tool(ToolCall::new(
                    "fs.write",
                    json!({ "path": "src/main.rs", "content": "fn main() { println!(\"hi\"); }" }),
                )),
                Step::new(1, StepKind::FileEdit, Risk::Low).with_tool(ToolCall::new(
                    "fs.write",
                    json!({ "path": "new_file.txt", "content": "hello world" }),
                )),
            ],
        );

        let summary = executor.execute(&plan).unwrap();

        assert!(summary.is_success());
        assert_eq!(summary.file_changes.len(), 2);

        // First change: modification of existing file.
        assert_eq!(summary.file_changes[0].kind, FileChangeKind::Modified);
        assert_eq!(summary.file_changes[0].path, PathBuf::from("src/main.rs"));

        // Second change: creation of new file.
        assert_eq!(summary.file_changes[1].kind, FileChangeKind::Created);
        assert_eq!(summary.file_changes[1].path, PathBuf::from("new_file.txt"));

        // No intercepted external actions for safe file edits.
        // (ModelQuery/CommandExec/ToolCall steps are recorded as "would execute"
        // but this plan has none of those.)
        assert!(!summary.has_intercepted_actions());
    }

    #[test]
    fn irreversible_action_is_intercepted_not_performed() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();
        let executor = ShadowExecutor::new(&sandbox);

        let plan = Plan::new(
            SessionId::new(),
            vec![Step::new(0, StepKind::ExternalAction, Risk::High)
                .with_tool(ToolCall::new(
                    "http.post",
                    json!({ "url": "https://api.example.com/deploy", "body": "{}" }),
                ))
                .irreversible()],
        );

        let summary = executor.execute(&plan).unwrap();

        assert!(summary.is_success());
        assert!(summary.has_intercepted_actions());
        assert_eq!(summary.intercepted_actions.len(), 1);
        assert_eq!(summary.intercepted_actions[0].step_seq, 0);
        assert!(summary.intercepted_actions[0]
            .description
            .contains("irreversible"));
    }

    #[test]
    fn external_action_without_irreversible_flag_is_still_intercepted() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();
        let executor = ShadowExecutor::new(&sandbox);

        // ExternalAction kind is intercepted even without the irreversible flag.
        let plan = Plan::new(
            SessionId::new(),
            vec![
                Step::new(0, StepKind::ExternalAction, Risk::Medium).with_tool(ToolCall::new(
                    "slack.send",
                    json!({ "channel": "#general", "text": "hi" }),
                )),
            ],
        );

        let summary = executor.execute(&plan).unwrap();

        assert!(summary.is_success());
        assert!(summary.has_intercepted_actions());
        assert_eq!(
            summary.intercepted_actions[0].description,
            "external action"
        );
    }

    #[test]
    fn failed_step_stops_execution_and_reports_failure() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();
        let executor = ShadowExecutor::new(&sandbox);

        // A FileEdit step with no tool call will fail.
        let plan = Plan::new(
            SessionId::new(),
            vec![
                Step::new(0, StepKind::FileEdit, Risk::Low), // no tool → will fail
                Step::new(1, StepKind::FileEdit, Risk::Low).with_tool(ToolCall::new(
                    "fs.write",
                    json!({ "path": "should_not_exist.txt", "content": "nope" }),
                )),
            ],
        );

        let summary = executor.execute(&plan).unwrap();

        // Execution should have stopped at step 0.
        assert!(!summary.is_success());
        assert_eq!(summary.failed_step.as_ref().unwrap().seq, 0);
        // Step 1 should NOT have been executed.
        assert!(summary.file_changes.is_empty());
    }

    #[test]
    fn sandbox_unchanged_after_external_action_shadow_execution() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();

        // Snapshot the sandbox state before execution.
        let main_before = fs::read_to_string(sandbox.root().join("src/main.rs")).unwrap();

        let executor = ShadowExecutor::new(&sandbox);

        let plan = Plan::new(
            SessionId::new(),
            vec![
                Step::new(0, StepKind::ExternalAction, Risk::High)
                    .with_tool(ToolCall::new(
                        "http.post",
                        json!({ "url": "https://api.example.com/deploy" }),
                    ))
                    .irreversible(),
                Step::new(1, StepKind::ExternalAction, Risk::Medium)
                    .with_tool(ToolCall::new("email.send", json!({ "to": "a@b.com" }))),
            ],
        );

        let summary = executor.execute(&plan).unwrap();
        assert!(summary.is_success());

        // The sandbox workspace should be completely unchanged.
        let main_after = fs::read_to_string(sandbox.root().join("src/main.rs")).unwrap();
        assert_eq!(main_before, main_after);

        // No new files should have been created.
        assert!(!sandbox.root().join("deploy_result.txt").exists());
    }

    #[test]
    fn shadow_execution_config_mandatory_forces_execution() {
        let session_id = SessionId::new();

        // A plan with no irreversible actions.
        let safe_plan = Plan::new(
            session_id,
            vec![
                Step::new(0, StepKind::FileEdit, Risk::Low).with_tool(ToolCall::new(
                    "fs.write",
                    json!({ "path": "a.txt", "content": "x" }),
                )),
            ],
        );

        let default_config = ShadowExecutionConfig::default();
        assert!(!default_config.should_shadow_execute(&safe_plan));

        let mandatory_config = ShadowExecutionConfig::mandatory();
        assert!(mandatory_config.should_shadow_execute(&safe_plan));
    }

    #[test]
    fn shadow_execution_config_triggers_on_irreversible_plan() {
        let session_id = SessionId::new();

        let irreversible_plan = Plan::new(
            session_id,
            vec![Step::new(0, StepKind::ExternalAction, Risk::High)
                .with_tool(ToolCall::new("deploy", json!({})))
                .irreversible()],
        );

        let default_config = ShadowExecutionConfig::default();
        assert!(default_config.should_shadow_execute(&irreversible_plan));
    }

    #[test]
    fn display_impl_renders_readable_summary() {
        let summary = ProjectedOutcomeSummary {
            file_changes: vec![
                FileChange {
                    path: PathBuf::from("src/main.rs"),
                    kind: FileChangeKind::Modified,
                    before_size: 12,
                    after_size: 30,
                },
                FileChange {
                    path: PathBuf::from("new.txt"),
                    kind: FileChangeKind::Created,
                    before_size: 0,
                    after_size: 11,
                },
            ],
            intercepted_actions: vec![InterceptedAction {
                step_seq: 2,
                description: "external action".to_string(),
                args: "http.post({\"url\":\"x\"})".to_string(),
            }],
            failed_step: None,
        };

        let rendered = format!("{summary}");
        assert!(rendered.contains("Projected Outcome Summary"));
        assert!(rendered.contains("src/main.rs"));
        assert!(rendered.contains("modified"));
        assert!(rendered.contains("new.txt"));
        assert!(rendered.contains("created"));
        assert!(rendered.contains("Intercepted external actions (1)"));
        assert!(rendered.contains("step 2"));
        assert!(rendered.contains("All steps completed successfully"));
    }

    #[test]
    fn file_delete_operation_records_deletion() {
        let workspace = create_test_workspace();
        let sandbox = Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap();
        let executor = ShadowExecutor::new(&sandbox);

        let plan = Plan::new(
            SessionId::new(),
            vec![
                Step::new(0, StepKind::FileEdit, Risk::Low).with_tool(ToolCall::new(
                    "fs.delete",
                    json!({ "path": "src/main.rs", "delete": true }),
                )),
            ],
        );

        let summary = executor.execute(&plan).unwrap();

        assert!(summary.is_success());
        assert_eq!(summary.file_changes.len(), 1);
        assert_eq!(summary.file_changes[0].kind, FileChangeKind::Deleted);
        assert!(summary.file_changes[0].before_size > 0);
        assert_eq!(summary.file_changes[0].after_size, 0);

        // The file should be gone from the sandbox.
        let sandbox_path = sandbox.translate_path(Path::new("src/main.rs")).unwrap();
        assert!(!sandbox_path.exists());
    }
}
