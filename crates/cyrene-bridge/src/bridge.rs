//! The Workspace_Bridge: one coordinated context across browser, terminal/FS,
//! and cloud repositories (R8).
//!
//! The bridge lets the Agent_Loop read a symptom in one context (e.g. a browser
//! console error) and apply the fix in another (e.g. a file in the local
//! workspace or a cloud repo), recording the cross-context action for the
//! ledger (R8.2, R8.3). Every filesystem-targeting action is checked against
//! the [`WorkspaceBoundary`]; out-of-bounds access is denied and recorded
//! (R8.4, R8.5).
//!
//! Ledger writes are abstracted behind the [`CrossContextLog`] trait so the
//! bridge stays decoupled from `cyrene-ledger` and is unit-testable.

use std::collections::VecDeque;

use crate::boundary::WorkspaceBoundary;
use crate::error::BridgeError;

/// The connected contexts the bridge coordinates (R8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// A connected browser session.
    Browser,
    /// The local terminal and filesystem.
    Terminal,
    /// A configured cloud repository.
    CloudRepo,
}

impl Context {
    /// A short label for logging.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Terminal => "terminal",
            Self::CloudRepo => "cloud-repo",
        }
    }
}

/// A captured browser console line, surfaced to the loop on request (R8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    /// The console level (e.g. `"error"`, `"warn"`, `"log"`).
    pub level: String,
    /// The message text.
    pub message: String,
}

impl ConsoleLine {
    /// Creates a console line.
    pub fn new(level: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: level.into(),
            message: message.into(),
        }
    }
}

/// A cross-context action the bridge performed, recorded for the ledger (R8.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossContextAction {
    /// The context the issue was observed in.
    pub from: Context,
    /// The context the fix was applied in.
    pub to: Context,
    /// A human-readable description of the change.
    pub description: String,
    /// The target path of the change (for filesystem contexts), if any.
    pub target: Option<String>,
}

/// Records cross-context actions and denied access attempts in the
/// Receipt_Ledger (R8.3, R8.5). Implemented by a ledger adapter in the runtime.
pub trait CrossContextLog {
    /// Records a successful cross-context action.
    fn record_action(&self, action: &CrossContextAction);

    /// Records a denied out-of-bounds access attempt.
    fn record_denied(&self, target: &str, context: Context);
}

/// The Workspace_Bridge coordinating the connected contexts.
pub struct WorkspaceBridge<L> {
    boundary: WorkspaceBoundary,
    log: L,
    /// Buffered browser console output, newest last.
    console: VecDeque<ConsoleLine>,
}

impl<L: CrossContextLog> WorkspaceBridge<L> {
    /// Creates a bridge over a workspace boundary and a cross-context log.
    pub fn new(boundary: WorkspaceBoundary, log: L) -> Self {
        Self {
            boundary,
            log,
            console: VecDeque::new(),
        }
    }

    /// Ingests a browser console line from a connected browser session.
    pub fn ingest_console(&mut self, line: ConsoleLine) {
        self.console.push_back(line);
    }

    /// Surfaces the captured browser console output to the loop (R8.2),
    /// optionally filtering by level (e.g. only `"error"`).
    #[must_use]
    pub fn console_output(&self, level: Option<&str>) -> Vec<ConsoleLine> {
        self.console
            .iter()
            .filter(|l| level.is_none_or(|lvl| l.level == lvl))
            .cloned()
            .collect()
    }

    /// Applies a cross-context fix: an issue observed in `from` is resolved by a
    /// change in `to`. When the target context is filesystem-backed
    /// (terminal/cloud-repo), `target` is checked against the workspace
    /// boundary; an out-of-bounds target is denied and recorded (R8.4, R8.5).
    /// On success the action is recorded for the ledger (R8.3).
    ///
    /// # Errors
    /// Returns [`BridgeError::OutOfBounds`] if `target` is outside the
    /// workspace boundary.
    pub fn apply_cross_context_fix(
        &self,
        from: Context,
        to: Context,
        description: impl Into<String>,
        target: Option<String>,
    ) -> Result<CrossContextAction, BridgeError> {
        // Enforce the boundary for filesystem-backed targets.
        if matches!(to, Context::Terminal | Context::CloudRepo) {
            if let Some(path) = &target {
                if !self.boundary.allows(path) {
                    self.log.record_denied(path, to);
                    return Err(BridgeError::OutOfBounds(path.clone()));
                }
            }
        }

        let action = CrossContextAction {
            from,
            to,
            description: description.into(),
            target,
        };
        self.log.record_action(&action);
        Ok(action)
    }

    /// Reads a file within the workspace boundary (terminal context).
    ///
    /// Enforces the boundary before touching the filesystem (R8.4); an
    /// out-of-bounds read is denied and recorded (R8.5).
    ///
    /// # Errors
    /// Returns [`BridgeError::OutOfBounds`] if `path` is outside the boundary,
    /// or [`BridgeError::Io`] if the read fails.
    pub fn read_workspace_file(&self, path: &str) -> Result<String, BridgeError> {
        if !self.boundary.allows(path) {
            self.log.record_denied(path, Context::Terminal);
            return Err(BridgeError::OutOfBounds(path.to_owned()));
        }
        std::fs::read_to_string(path).map_err(|e| BridgeError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct RecordingLog {
        actions: RefCell<Vec<CrossContextAction>>,
        denials: RefCell<Vec<(String, Context)>>,
    }
    impl CrossContextLog for RecordingLog {
        fn record_action(&self, action: &CrossContextAction) {
            self.actions.borrow_mut().push(action.clone());
        }
        fn record_denied(&self, target: &str, context: Context) {
            self.denials.borrow_mut().push((target.to_owned(), context));
        }
    }

    fn bridge() -> WorkspaceBridge<RecordingLog> {
        let boundary = WorkspaceBoundary::new(["/home/alice/project"]);
        WorkspaceBridge::new(boundary, RecordingLog::default())
    }

    #[test]
    fn surfaces_browser_console_output_filtered_by_level() {
        let mut b = bridge();
        b.ingest_console(ConsoleLine::new("log", "starting"));
        b.ingest_console(ConsoleLine::new("error", "TypeError: x is undefined"));
        b.ingest_console(ConsoleLine::new("warn", "deprecated API"));

        let errors = b.console_output(Some("error"));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("TypeError"));

        assert_eq!(b.console_output(None).len(), 3);
    }

    #[test]
    fn cross_context_fix_inside_boundary_is_applied_and_logged() {
        let b = bridge();
        let action = b
            .apply_cross_context_fix(
                Context::Browser,
                Context::Terminal,
                "fix undefined var in source",
                Some("/home/alice/project/src/app.js".to_owned()),
            )
            .unwrap();
        assert_eq!(action.from, Context::Browser);
        assert_eq!(action.to, Context::Terminal);
        assert_eq!(b.log.actions.borrow().len(), 1);
        assert!(b.log.denials.borrow().is_empty());
    }

    #[test]
    fn out_of_bounds_fix_is_denied_and_recorded() {
        let b = bridge();
        let err = b
            .apply_cross_context_fix(
                Context::Browser,
                Context::Terminal,
                "write outside workspace",
                Some("/etc/cron.d/evil".to_owned()),
            )
            .unwrap_err();
        assert!(matches!(err, BridgeError::OutOfBounds(_)));
        // Nothing applied, but the denial was recorded (R8.5).
        assert!(b.log.actions.borrow().is_empty());
        assert_eq!(b.log.denials.borrow().len(), 1);
        assert_eq!(b.log.denials.borrow()[0].1, Context::Terminal);
    }

    #[test]
    fn browser_target_fix_skips_boundary_check() {
        // A fix applied back in the browser context has no filesystem target,
        // so the boundary check does not apply.
        let b = bridge();
        let action = b
            .apply_cross_context_fix(Context::Terminal, Context::Browser, "reload tab", None)
            .unwrap();
        assert_eq!(action.to, Context::Browser);
    }

    #[test]
    fn read_workspace_file_enforces_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();

        let boundary = WorkspaceBoundary::new([dir.path()]);
        let b = WorkspaceBridge::new(boundary, RecordingLog::default());

        let content = b.read_workspace_file(file.to_str().unwrap()).unwrap();
        assert_eq!(content, "hello");

        // A read outside the boundary is denied + recorded.
        let err = b.read_workspace_file("/etc/passwd").unwrap_err();
        assert!(matches!(err, BridgeError::OutOfBounds(_)));
        assert_eq!(b.log.denials.borrow().len(), 1);
    }

    #[test]
    fn cloud_repo_target_is_boundary_checked_too() {
        let b = bridge();
        // Cloud-repo path outside the boundary is denied.
        let err = b
            .apply_cross_context_fix(
                Context::Browser,
                Context::CloudRepo,
                "patch",
                Some("/tmp/not-workspace/file".to_owned()),
            )
            .unwrap_err();
        assert!(matches!(err, BridgeError::OutOfBounds(_)));
    }
}
