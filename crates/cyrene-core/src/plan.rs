//! Plans and steps.
//!
//! A [`Plan`] is the ordered set of [`Step`]s the Agent_Loop intends to execute
//! for a request (per the glossary). Each step has a [`StepKind`], a [`Risk`]
//! classification, an optional [`ToolCall`], and an `irreversible` flag that the
//! Approval_Gate and Shadow_Executor key off of (R3, R6).

use serde::{Deserialize, Serialize};

use crate::ids::{PlanId, SessionId};
use crate::risk::Risk;

/// An invocation of a named tool with structured arguments.
///
/// Arguments are held as free-form JSON so the type does not need to know every
/// tool's schema; concrete tools validate their own arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The tool's stable name, e.g. `"fs.write"` or `"shell.run"`.
    pub name: String,
    /// Arguments to the tool, as a JSON object/value.
    pub args: serde_json::Value,
}

impl ToolCall {
    /// Creates a tool call with the given name and arguments.
    pub fn new(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }
}

/// The category of action a [`Step`] performs.
///
/// Mirrors the glossary's examples of a step ("a tool call, file edit, or model
/// query") and the kinds of irreversible action called out in R3/R6 (external
/// messages, deploys, financial transactions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StepKind {
    /// A completion/reasoning call to a Model_Provider.
    ModelQuery,
    /// A generic tool invocation.
    ToolCall,
    /// A change to a file in the workspace.
    FileEdit,
    /// A local command/process execution.
    CommandExec,
    /// An action that affects a resource outside the workspace (network call,
    /// message to a third party, deploy, transaction).
    ExternalAction,
}

/// A single discrete action within a [`Plan`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// Position of this step within its plan, starting at 0.
    pub seq: u64,
    /// What kind of action this step performs.
    pub kind: StepKind,
    /// The risk classification assigned by the autonomy policy.
    pub risk: Risk,
    /// The tool invoked by this step, if any.
    pub tool: Option<ToolCall>,
    /// Whether this step's effect cannot be automatically undone (an
    /// Irreversible_Action). Such steps route through the Approval_Gate.
    pub irreversible: bool,
}

impl Step {
    /// Creates a step. `tool` and `irreversible` default to none/`false`; use
    /// the builder-style setters to refine.
    #[must_use]
    pub fn new(seq: u64, kind: StepKind, risk: Risk) -> Self {
        Self {
            seq,
            kind,
            risk,
            tool: None,
            irreversible: false,
        }
    }

    /// Attaches a [`ToolCall`] to this step.
    #[must_use]
    pub fn with_tool(mut self, tool: ToolCall) -> Self {
        self.tool = Some(tool);
        self
    }

    /// Marks this step as irreversible.
    #[must_use]
    pub fn irreversible(mut self) -> Self {
        self.irreversible = true;
        self
    }
}

/// An ordered set of steps the Agent_Loop intends to execute for a request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// Unique identifier for this plan.
    pub id: PlanId,
    /// The session this plan belongs to.
    pub session_id: SessionId,
    /// The ordered steps, by `seq`.
    pub steps: Vec<Step>,
}

impl Plan {
    /// Creates a plan with a fresh id for the given session.
    #[must_use]
    pub fn new(session_id: SessionId, steps: Vec<Step>) -> Self {
        Self {
            id: PlanId::new(),
            session_id,
            steps,
        }
    }

    /// Returns `true` if any step in the plan is an Irreversible_Action.
    ///
    /// The Agent_Loop uses this to decide whether shadow execution and the
    /// Approval_Gate are required for the plan (R3.1).
    #[must_use]
    pub fn has_irreversible_action(&self) -> bool {
        self.steps.iter().any(|step| step.irreversible)
    }
}

#[cfg(test)]
mod tests {
    use super::{Plan, Step, StepKind, ToolCall};
    use crate::ids::SessionId;
    use crate::risk::Risk;
    use serde_json::json;

    #[test]
    fn step_new_defaults_to_no_tool_and_reversible() {
        let step = Step::new(0, StepKind::ModelQuery, Risk::Low);
        assert_eq!(step.seq, 0);
        assert_eq!(step.kind, StepKind::ModelQuery);
        assert_eq!(step.risk, Risk::Low);
        assert!(step.tool.is_none());
        assert!(!step.irreversible);
    }

    #[test]
    fn step_with_tool_attaches_tool_call() {
        let call = ToolCall::new("fs.write", json!({ "path": "a.txt" }));
        let step = Step::new(1, StepKind::FileEdit, Risk::Medium).with_tool(call.clone());
        assert_eq!(step.tool, Some(call));
    }

    #[test]
    fn step_irreversible_sets_flag() {
        let step = Step::new(2, StepKind::ExternalAction, Risk::High).irreversible();
        assert!(step.irreversible);
    }

    #[test]
    fn step_builders_chain() {
        let step = Step::new(3, StepKind::CommandExec, Risk::Medium)
            .with_tool(ToolCall::new("shell.run", json!({ "cmd": "ls" })))
            .irreversible();
        assert!(step.tool.is_some());
        assert!(step.irreversible);
    }

    #[test]
    fn plan_new_assigns_session_and_steps() {
        let session_id = SessionId::new();
        let steps = vec![Step::new(0, StepKind::ModelQuery, Risk::Low)];
        let plan = Plan::new(session_id, steps.clone());
        assert_eq!(plan.session_id, session_id);
        assert_eq!(plan.steps, steps);
    }

    #[test]
    fn has_irreversible_action_false_when_all_reversible() {
        let plan = Plan::new(
            SessionId::new(),
            vec![
                Step::new(0, StepKind::ModelQuery, Risk::Low),
                Step::new(1, StepKind::FileEdit, Risk::Low),
            ],
        );
        assert!(!plan.has_irreversible_action());
    }

    #[test]
    fn has_irreversible_action_false_for_empty_plan() {
        let plan = Plan::new(SessionId::new(), vec![]);
        assert!(!plan.has_irreversible_action());
    }

    #[test]
    fn has_irreversible_action_true_when_any_step_irreversible() {
        let plan = Plan::new(
            SessionId::new(),
            vec![
                Step::new(0, StepKind::ModelQuery, Risk::Low),
                Step::new(1, StepKind::ExternalAction, Risk::High).irreversible(),
                Step::new(2, StepKind::FileEdit, Risk::Low),
            ],
        );
        assert!(plan.has_irreversible_action());
    }

    #[test]
    fn tool_call_round_trip() {
        let call = ToolCall::new("shell.run", json!({ "cmd": "echo hi", "n": 1 }));
        let json_str = serde_json::to_string(&call).unwrap();
        let back: ToolCall = serde_json::from_str(&json_str).unwrap();
        assert_eq!(call, back);
    }

    #[test]
    fn step_round_trip() {
        let step = Step::new(7, StepKind::ExternalAction, Risk::High)
            .with_tool(ToolCall::new("http.post", json!({ "url": "x" })))
            .irreversible();
        let json_str = serde_json::to_string(&step).unwrap();
        let back: Step = serde_json::from_str(&json_str).unwrap();
        assert_eq!(step, back);
    }

    #[test]
    fn plan_round_trip() {
        let plan = Plan::new(
            SessionId::new(),
            vec![
                Step::new(0, StepKind::ModelQuery, Risk::Low),
                Step::new(1, StepKind::ExternalAction, Risk::High).irreversible(),
            ],
        );
        let json_str = serde_json::to_string(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json_str).unwrap();
        assert_eq!(plan, back);
    }

    #[test]
    fn step_kind_round_trip_each_variant() {
        for kind in [
            StepKind::ModelQuery,
            StepKind::ToolCall,
            StepKind::FileEdit,
            StepKind::CommandExec,
            StepKind::ExternalAction,
        ] {
            let json_str = serde_json::to_string(&kind).unwrap();
            let back: StepKind = serde_json::from_str(&json_str).unwrap();
            assert_eq!(kind, back);
        }
    }
}
