//! The Agent_Loop: the spine that composes Cyrene's safety pipeline (R3–R6,
//! R12, R21).
//!
//! A single turn flows through a fixed, unskippable pipeline:
//!
//! ```text
//! inbound → injection scan → plan → shadow (if irreversible) →
//!   approval gate → execute → checkpoint → signed receipt → response
//! ```
//!
//! Safety, auditing, and approval are composed into the loop rather than bolted
//! on, so they cannot be bypassed:
//!
//! - **Injection scan (R21):** untrusted inbound content is scanned first;
//!   quarantined content is recorded and the turn refuses to plan from it.
//! - **Plan (R12):** a [`Planner`] (the Model_Router boundary) turns the request
//!   into a [`Plan`]. An intent receipt is appended before any step runs.
//! - **Shadow (R3):** if the plan contains an irreversible action (or shadowing
//!   is mandatory), it is dry-run in a sandbox producing a
//!   [`ProjectedOutcomeSummary`]; a failed shadow withholds real execution.
//! - **Approval gate (R6):** irreversible steps halt for Approve / Rewrite /
//!   Abort before any real side effect.
//! - **Execute (R3):** approved/safe steps run through an [`Executor`].
//! - **Checkpoint (R4) + signed receipt (R5):** every executed step records a
//!   State_Tree checkpoint and a signed, hash-chained receipt.
//!
//! The model-planning and real-execution boundaries are expressed as the
//! [`Planner`] and [`Executor`] traits so the loop is testable in isolation and
//! the concrete model/tool crates wire in from the outside.

use cyrene_core::{Plan, Session, Step};
use cyrene_ledger::{digest_inputs, Ledger};
use cyrene_safety::{
    ApprovalGate, ApprovalRequest, ApprovalResponse, AutonomyDecision, AutonomyPolicy,
    ContentSource, InjectionScanner, Sandbox, ScanResult, ShadowExecutionConfig, ShadowExecutor,
};
use cyrene_state::StateStore;

use crate::error::LoopError;

/// Produces a [`Plan`] for a request. This is the Model_Router boundary (R12):
/// in production a router-backed planner calls a model; in tests a fake returns
/// a fixed plan.
pub trait Planner {
    /// Turns a clean request into a plan for the given session.
    ///
    /// # Errors
    /// Returns a planner-specific error string if no plan can be produced.
    fn plan(&self, session: &Session, request: &str) -> Result<Plan, String>;
}

/// The result of really executing a single step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutput {
    /// A human-readable result of the step.
    pub result: String,
    /// Files the step changed, as `(relative_path, content_bytes)` pairs, so
    /// the loop can checkpoint exact bytes (R4.2).
    pub files: Vec<(String, Vec<u8>)>,
}

impl StepOutput {
    /// A result with no file changes.
    #[must_use]
    pub fn message(result: impl Into<String>) -> Self {
        Self {
            result: result.into(),
            files: Vec::new(),
        }
    }
}

/// Performs the real side effect of a step. This is the execution boundary
/// (tools/bridge); the loop only calls it for approved or auto-safe steps.
pub trait Executor {
    /// Executes a step for real and returns its output.
    ///
    /// # Errors
    /// Returns an error string if the real action fails.
    fn execute(&mut self, step: &Step) -> Result<StepOutput, String>;
}

/// Decides how the human responds to an approval request. This is the
/// human-in-the-loop boundary (a channel prompt in production).
pub trait ApprovalResponder {
    /// Returns the user's response to a pending approval request.
    fn respond(&mut self, request: &ApprovalRequest) -> ApprovalResponse;
}

/// What happened to a single step as the loop processed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepDisposition {
    /// The step was executed for real (carries its result).
    Executed(String),
    /// The step was blocked by autonomy policy (carries the reason).
    Blocked(String),
    /// The step was aborted at the approval gate by the user.
    Aborted,
    /// The user returned corrective instructions instead of approving (R6.5).
    Rewritten(String),
}

/// The outcome of running one turn through the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    /// Whether the inbound content was quarantined (turn refused, R21.3).
    pub quarantined: bool,
    /// The disposition of each plan step, in order.
    pub steps: Vec<StepDisposition>,
    /// The projected outcome summary, if the plan was shadow-executed.
    pub shadow_summary: Option<String>,
    /// The number of receipts appended to the ledger this turn.
    pub receipts: u64,
    /// The number of checkpoints recorded this turn.
    pub checkpoints: u64,
    /// The final response text assembled from executed steps.
    pub response: String,
}

/// The Agent_Loop. Owns the safety/audit substrates and composes them into the
/// request lifecycle.
pub struct AgentLoop<'a> {
    scanner: &'a InjectionScanner,
    policy: &'a AutonomyPolicy,
    shadow_cfg: ShadowExecutionConfig,
    sandbox: &'a Sandbox,
    gate: &'a mut ApprovalGate,
    ledger: &'a Ledger,
    state: &'a StateStore,
}

impl<'a> AgentLoop<'a> {
    /// Composes the loop from its safety, audit, and approval substrates.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scanner: &'a InjectionScanner,
        policy: &'a AutonomyPolicy,
        shadow_cfg: ShadowExecutionConfig,
        sandbox: &'a Sandbox,
        gate: &'a mut ApprovalGate,
        ledger: &'a Ledger,
        state: &'a StateStore,
    ) -> Self {
        Self {
            scanner,
            policy,
            shadow_cfg,
            sandbox,
            gate,
            ledger,
            state,
        }
    }

    /// Runs one turn for `request` on `session`, driving the full pipeline.
    ///
    /// `source` classifies the inbound content's trust (R21.1). A turn from
    /// untrusted, quarantined content is refused before planning.
    ///
    /// # Errors
    /// Returns [`LoopError`] if the planner, an executor, the ledger, the state
    /// store, the sandbox, or the approval gate fails.
    pub fn run_turn<P, E, R>(
        &mut self,
        session: &Session,
        request: &str,
        source: ContentSource,
        planner: &P,
        executor: &mut E,
        responder: &mut R,
    ) -> Result<TurnOutcome, LoopError>
    where
        P: Planner,
        E: Executor,
        R: ApprovalResponder,
    {
        let mut receipts = 0u64;
        let mut checkpoints = 0u64;

        // ── Step 1: injection scan of untrusted inbound content (R21). ──
        let scan = self.scanner.scan(request, source);
        if let ScanResult::Quarantined { detections, .. } = &scan {
            // Record the refusal in the ledger; never plan from quarantined input.
            self.ledger.append(
                session.id,
                "injection-quarantine",
                digest_inputs(request),
                "injection-scanner",
            )?;
            receipts += 1;
            let _ = detections;
            return Ok(TurnOutcome {
                quarantined: true,
                steps: Vec::new(),
                shadow_summary: None,
                receipts,
                checkpoints,
                response: "Request quarantined: untrusted content contained \
                           injection patterns and was not executed."
                    .to_owned(),
            });
        }

        // ── Step 2: plan the request (Model_Router boundary, R12). ──
        let plan = planner
            .plan(session, scan.content())
            .map_err(LoopError::Planner)?;

        // Append an intent receipt for the plan before any step runs (R5.1).
        self.ledger
            .append(session.id, "plan", digest_inputs(request), "model-router")?;
        receipts += 1;

        // ── Step 3: shadow-execute if required (R3). ──
        let mut shadow_summary = None;
        if self.shadow_cfg.should_shadow_execute(&plan) {
            let executor = ShadowExecutor::new(self.sandbox);
            let summary = executor.execute(&plan)?;
            shadow_summary = Some(summary.to_string());

            self.ledger.append(
                session.id,
                "shadow-execute",
                digest_inputs(summary.to_string()),
                "shadow-executor",
            )?;
            receipts += 1;

            // A failed shadow withholds all real execution (R3.5).
            if !summary.is_success() {
                return Ok(TurnOutcome {
                    quarantined: false,
                    steps: Vec::new(),
                    shadow_summary,
                    receipts,
                    checkpoints,
                    response: "Plan withheld: shadow execution failed before any \
                               real side effect."
                        .to_owned(),
                });
            }
        }

        // ── Steps 4–6: per-step gate → execute → checkpoint → receipt. ──
        let mut dispositions = Vec::with_capacity(plan.steps.len());
        let mut response_parts = Vec::new();

        for step in &plan.steps {
            match self.process_step(
                session,
                step,
                executor,
                responder,
                &mut receipts,
                &mut checkpoints,
            )? {
                StepDisposition::Executed(result) => {
                    response_parts.push(result.clone());
                    dispositions.push(StepDisposition::Executed(result));
                }
                // Blocked / Aborted halt the plan: stop processing further steps.
                blocked_or_aborted => {
                    let stop = matches!(
                        blocked_or_aborted,
                        StepDisposition::Blocked(_) | StepDisposition::Aborted
                    );
                    dispositions.push(blocked_or_aborted);
                    if stop {
                        break;
                    }
                }
            }
        }

        let response = if response_parts.is_empty() {
            "No steps were executed.".to_owned()
        } else {
            response_parts.join("\n")
        };

        Ok(TurnOutcome {
            quarantined: false,
            steps: dispositions,
            shadow_summary,
            receipts,
            checkpoints,
            response,
        })
    }

    /// Processes one step: autonomy gate → (approval if irreversible) →
    /// execute → checkpoint → signed receipt.
    fn process_step<E, R>(
        &mut self,
        session: &Session,
        step: &Step,
        executor: &mut E,
        responder: &mut R,
        receipts: &mut u64,
        checkpoints: &mut u64,
    ) -> Result<StepDisposition, LoopError>
    where
        E: Executor,
        R: ApprovalResponder,
    {
        // Autonomy policy decision based on the step's risk (R22).
        match self.policy.evaluate(step) {
            AutonomyDecision::Blocked { reason } => {
                self.ledger.append(
                    session.id,
                    "step-blocked",
                    digest_inputs(format!("step {}", step.seq)),
                    "autonomy-policy",
                )?;
                *receipts += 1;
                return Ok(StepDisposition::Blocked(reason));
            }
            AutonomyDecision::RequiresApproval { reason } => {
                // Halt for human approval before any real side effect (R6).
                let request = ApprovalRequest::new(
                    step.seq,
                    format!("{:?} step requires approval", step.kind),
                    reason,
                );
                let request = self.gate.request_approval(request).request.clone();
                let response = responder.respond(&request);
                self.gate.resolve(request.id, response.clone())?;

                match response {
                    ApprovalResponse::Abort => {
                        self.ledger.append(
                            session.id,
                            "step-aborted",
                            digest_inputs(format!("step {}", step.seq)),
                            "approval-gate",
                        )?;
                        *receipts += 1;
                        return Ok(StepDisposition::Aborted);
                    }
                    ApprovalResponse::Rewrite { instructions } => {
                        self.ledger.append(
                            session.id,
                            "step-rewritten",
                            digest_inputs(&instructions),
                            "approval-gate",
                        )?;
                        *receipts += 1;
                        return Ok(StepDisposition::Rewritten(instructions));
                    }
                    ApprovalResponse::Approve => { /* fall through to execute */ }
                }
            }
            AutonomyDecision::Proceed => { /* auto-execute */ }
        }

        // Execute the step for real (only reached when approved or auto-safe).
        let output = executor
            .execute(step)
            .map_err(|message| LoopError::Executor {
                seq: step.seq,
                message,
            })?;

        // Checkpoint the executed step's file state (R4).
        let file_refs: Vec<(&str, &[u8])> = output
            .files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_slice()))
            .collect();
        self.state.checkpoint(
            session.id,
            step.seq,
            None,
            session.branch_id,
            format!("step {} executed", step.seq),
            b"",
            b"",
            &file_refs,
        )?;
        *checkpoints += 1;

        // Append a signed outcome receipt for the executed step (R5).
        self.ledger.append(
            session.id,
            "step-executed",
            digest_inputs(&output.result),
            "executor",
        )?;
        *receipts += 1;

        Ok(StepDisposition::Executed(output.result))
    }
}
