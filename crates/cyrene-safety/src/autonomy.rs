//! Runtime autonomy enforcement (R22).
//!
//! This module implements the *runtime* side of the autonomy policy. The
//! *configuration* side lives in [`cyrene_config::AutonomyConfig`]; this module
//! consumes that config to classify steps, evaluate decisions, and gate
//! commands at execution time.
//!
//! ## Components
//!
//! - [`RiskClassifier`] — assigns a [`Risk`] level to a [`Step`] based on its
//!   [`StepKind`] and properties (e.g. `irreversible` flag).
//! - [`AutonomyPolicy`] — wraps an [`AutonomyConfig`] and provides the
//!   `evaluate`, `is_command_allowed`, and `gate_command` entry points.
//! - [`AutonomyDecision`] — the outcome of a policy evaluation: proceed
//!   automatically, require approval, or block entirely.

use cyrene_config::{AutonomyAction, AutonomyConfig};
use cyrene_core::Risk;
use cyrene_core::{Step, StepKind};

// ─── AutonomyDecision ────────────────────────────────────────────────────────

/// The outcome of evaluating a step or command against the autonomy policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutonomyDecision {
    /// Execute automatically without user involvement.
    Proceed,
    /// Halt and request user approval before executing.
    RequiresApproval { reason: String },
    /// Refuse to execute entirely.
    Blocked { reason: String },
}

// ─── RiskClassifier ──────────────────────────────────────────────────────────

/// Classifies a [`Step`] into a [`Risk`] level.
///
/// Default classification by [`StepKind`]:
/// - `FileEdit` → Low
/// - `ModelQuery` → Low
/// - `ToolCall` → Medium
/// - `CommandExec` → Medium
/// - `ExternalAction` → High
///
/// Steps marked `irreversible = true` are **always** classified as [`Risk::High`]
/// regardless of their kind.
#[derive(Debug, Clone, Copy)]
pub struct RiskClassifier;

impl RiskClassifier {
    /// Classify the risk of a step.
    #[must_use]
    pub fn classify(step: &Step) -> Risk {
        if step.irreversible {
            return Risk::High;
        }
        Self::classify_kind(step.kind)
    }

    /// Classify risk purely by [`StepKind`], ignoring the irreversible flag.
    #[must_use]
    pub fn classify_kind(kind: StepKind) -> Risk {
        match kind {
            StepKind::FileEdit | StepKind::ModelQuery => Risk::Low,
            StepKind::ToolCall | StepKind::CommandExec => Risk::Medium,
            StepKind::ExternalAction => Risk::High,
        }
    }
}

// ─── AutonomyPolicy ──────────────────────────────────────────────────────────

/// Runtime enforcement of the autonomy policy.
///
/// Wraps an [`AutonomyConfig`] (from `cyrene-config`) and maps classified risk
/// levels to [`AutonomyDecision`]s.
#[derive(Debug, Clone)]
pub struct AutonomyPolicy {
    config: AutonomyConfig,
}

impl AutonomyPolicy {
    /// Create a policy from the given configuration.
    #[must_use]
    pub fn new(config: AutonomyConfig) -> Self {
        Self { config }
    }

    /// Create a policy with the secure-by-default configuration (R22.1).
    #[must_use]
    pub fn default_policy() -> Self {
        Self::new(AutonomyConfig::default())
    }

    /// Evaluate a step against the policy.
    ///
    /// Classifies the step's risk, then maps it to a decision based on the
    /// configured action for that risk level.
    #[must_use]
    pub fn evaluate(&self, step: &Step) -> AutonomyDecision {
        let risk = RiskClassifier::classify(step);
        let action = self.config.action_for(risk);
        Self::action_to_decision(action, risk, &Self::step_description(step))
    }

    /// Check whether a command is on the configured allowlist.
    ///
    /// Delegates to [`AutonomyConfig::is_command_allowed`].
    #[must_use]
    pub fn is_command_allowed(&self, command: &str) -> bool {
        self.config.is_command_allowed(command)
    }

    /// Gate a command: if it is on the allowlist, return [`AutonomyDecision::Proceed`];
    /// otherwise return [`AutonomyDecision::RequiresApproval`] (R22.2).
    #[must_use]
    pub fn gate_command(&self, command: &str) -> AutonomyDecision {
        if self.is_command_allowed(command) {
            AutonomyDecision::Proceed
        } else {
            AutonomyDecision::RequiresApproval {
                reason: format!(
                    "command '{}' is not on the allowlist and requires approval",
                    command
                ),
            }
        }
    }

    /// Returns a reference to the underlying configuration.
    #[must_use]
    pub fn config(&self) -> &AutonomyConfig {
        &self.config
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn action_to_decision(action: AutonomyAction, risk: Risk, context: &str) -> AutonomyDecision {
        match action {
            AutonomyAction::Auto => AutonomyDecision::Proceed,
            AutonomyAction::Approval => AutonomyDecision::RequiresApproval {
                reason: format!("{risk:?}-risk step requires approval: {context}"),
            },
            AutonomyAction::Blocked => AutonomyDecision::Blocked {
                reason: format!("{risk:?}-risk step is blocked by policy: {context}"),
            },
        }
    }

    fn step_description(step: &Step) -> String {
        let kind_label = match step.kind {
            StepKind::ModelQuery => "model query",
            StepKind::ToolCall => "tool call",
            StepKind::FileEdit => "file edit",
            StepKind::CommandExec => "command execution",
            StepKind::ExternalAction => "external action",
        };
        if step.irreversible {
            format!("irreversible {kind_label}")
        } else {
            kind_label.to_string()
        }
    }
}

impl Default for AutonomyPolicy {
    fn default() -> Self {
        Self::default_policy()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use cyrene_core::Step;

    /// Helper: create a simple step with the given kind and irreversible flag.
    fn make_step(kind: StepKind, irreversible: bool) -> Step {
        let mut step = Step::new(0, kind, Risk::Low); // risk field on Step is ignored by classifier
        if irreversible {
            step = step.irreversible();
        }
        step
    }

    // ── RiskClassifier tests ─────────────────────────────────────────────────

    #[test]
    fn classify_file_edit_is_low() {
        let step = make_step(StepKind::FileEdit, false);
        assert_eq!(RiskClassifier::classify(&step), Risk::Low);
    }

    #[test]
    fn classify_model_query_is_low() {
        let step = make_step(StepKind::ModelQuery, false);
        assert_eq!(RiskClassifier::classify(&step), Risk::Low);
    }

    #[test]
    fn classify_tool_call_is_medium() {
        let step = make_step(StepKind::ToolCall, false);
        assert_eq!(RiskClassifier::classify(&step), Risk::Medium);
    }

    #[test]
    fn classify_command_exec_is_medium() {
        let step = make_step(StepKind::CommandExec, false);
        assert_eq!(RiskClassifier::classify(&step), Risk::Medium);
    }

    #[test]
    fn classify_external_action_is_high() {
        let step = make_step(StepKind::ExternalAction, false);
        assert_eq!(RiskClassifier::classify(&step), Risk::High);
    }

    #[test]
    fn irreversible_always_high_regardless_of_kind() {
        for kind in [
            StepKind::FileEdit,
            StepKind::ModelQuery,
            StepKind::ToolCall,
            StepKind::CommandExec,
            StepKind::ExternalAction,
        ] {
            let step = make_step(kind, true);
            assert_eq!(
                RiskClassifier::classify(&step),
                Risk::High,
                "irreversible {kind:?} should be High"
            );
        }
    }

    // ── AutonomyPolicy default tests ─────────────────────────────────────────

    #[test]
    fn default_policy_file_edit_proceeds() {
        let policy = AutonomyPolicy::default_policy();
        let step = make_step(StepKind::FileEdit, false);
        assert_eq!(policy.evaluate(&step), AutonomyDecision::Proceed);
    }

    #[test]
    fn default_policy_command_exec_requires_approval() {
        let policy = AutonomyPolicy::default_policy();
        let step = make_step(StepKind::CommandExec, false);
        assert!(matches!(
            policy.evaluate(&step),
            AutonomyDecision::RequiresApproval { .. }
        ));
    }

    #[test]
    fn default_policy_external_action_is_blocked() {
        let policy = AutonomyPolicy::default_policy();
        let step = make_step(StepKind::ExternalAction, false);
        assert!(matches!(
            policy.evaluate(&step),
            AutonomyDecision::Blocked { .. }
        ));
    }

    #[test]
    fn default_policy_irreversible_step_is_blocked() {
        let policy = AutonomyPolicy::default_policy();
        // Even a FileEdit, if irreversible, becomes High → Blocked.
        let step = make_step(StepKind::FileEdit, true);
        assert!(matches!(
            policy.evaluate(&step),
            AutonomyDecision::Blocked { .. }
        ));
    }

    // ── Command allowlist tests ──────────────────────────────────────────────

    #[test]
    fn non_allowlisted_command_requires_approval() {
        let policy = AutonomyPolicy::default_policy();
        let decision = policy.gate_command("rm -rf /");
        assert!(matches!(
            decision,
            AutonomyDecision::RequiresApproval { .. }
        ));
    }

    #[test]
    fn allowlisted_command_proceeds() {
        let config = AutonomyConfig {
            command_allowlist: vec!["git".to_string(), "cargo".to_string()],
            ..Default::default()
        };
        let policy = AutonomyPolicy::new(config);
        assert_eq!(policy.gate_command("git status"), AutonomyDecision::Proceed);
        assert_eq!(
            policy.gate_command("cargo build"),
            AutonomyDecision::Proceed
        );
    }

    #[test]
    fn is_command_allowed_delegates_to_config() {
        let config = AutonomyConfig {
            command_allowlist: vec!["ls".to_string()],
            ..Default::default()
        };
        let policy = AutonomyPolicy::new(config);
        assert!(policy.is_command_allowed("ls -la"));
        assert!(!policy.is_command_allowed("rm file.txt"));
    }

    // ── Raised autonomy tests (R22.4) ───────────────────────────────────────

    #[test]
    fn raised_autonomy_medium_auto_changes_decision() {
        // Simulates explicit config: medium = auto (R22.4 — requires explicit config).
        let config = AutonomyConfig {
            medium: AutonomyAction::Auto,
            ..Default::default()
        };
        let policy = AutonomyPolicy::new(config);
        let step = make_step(StepKind::CommandExec, false);
        // With raised autonomy, medium-risk steps now proceed automatically.
        assert_eq!(policy.evaluate(&step), AutonomyDecision::Proceed);
    }

    #[test]
    fn raised_autonomy_high_approval_changes_decision() {
        // Simulates explicit config: high = approval instead of blocked.
        let config = AutonomyConfig {
            high: AutonomyAction::Approval,
            ..Default::default()
        };
        let policy = AutonomyPolicy::new(config);
        let step = make_step(StepKind::ExternalAction, false);
        assert!(matches!(
            policy.evaluate(&step),
            AutonomyDecision::RequiresApproval { .. }
        ));
    }
}
