//! End-to-end smoke test for the install → onboard → request flow (Task 23.3).
//!
//! Proves the headline adoption promise: after a scripted, non-interactive
//! install + onboarding, the runtime can process a CLI request without further
//! configuration (R23.4), and a single request received on a channel is picked
//! up by the daemon's O(1) cold path (R1.3) and produces a signed Receipt and a
//! State_Tree Checkpoint (R5.1, R4.1).
//!
//! The flow it exercises, end to end:
//!
//! 1. **Scripted install + onboard.** The real `cyrene` binary (built by Cargo
//!    and located via `CARGO_BIN_EXE_cyrene`) is run as a subprocess with a
//!    throwaway `$HOME`, exactly as `install.sh` invokes it after building:
//!    `cyrene onboard --non-interactive` writes `~/.cyrene/config.toml`, then
//!    `cyrene doctor` reports the configured providers/channels (R23.2, R23.5).
//! 2. **Request.** The onboarded config is loaded back and used to wire the
//!    Agent_Loop from the same safety/audit substrates the daemon uses. A CLI
//!    `InboundRequest` is dispatched through the real [`Daemon`]; the handler
//!    runs one turn and writes a receipt + checkpoint into the very
//!    `~/.cyrene/cyrene.db` the `doctor` command points at.
//! 3. **Assert.** Reopening the persisted ledger and state store, the test
//!    confirms the receipt chain verifies and a checkpoint was recorded for the
//!    session — i.e. the onboarded runtime really processed the request.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cyrene_config::Config;
use cyrene_core::{
    Budget, ChannelOrigin, Plan, Risk, Session, SessionId, Step, StepKind, ToolCall, UserId,
};
use cyrene_ledger::{InstallKey, Ledger};
use cyrene_runtime::{
    AgentLoop, ApprovalResponder, Daemon, Executor, InboundRequest, Planner, RequestHandler,
    StepOutput,
};
use cyrene_safety::{
    ApprovalGate, ApprovalRequest, ApprovalResponse, AutonomyPolicy, ContentSource,
    InjectionScanner, Sandbox, SandboxBackend, ShadowExecutionConfig,
};
use cyrene_state::StateStore;
use serde_json::json;

// ─── Scripted install + onboard (drives the real binary) ────────────────────

/// Runs `cyrene onboard --non-interactive` with `$HOME` pointed at `home`,
/// mirroring what `install.sh` does after building the binary. Returns once the
/// onboarding has written `~/.cyrene/config.toml`.
fn run_scripted_onboard(home: &Path, provider: &str, channel: &str) {
    let status = cyrene_command(home)
        .args([
            "onboard",
            "--non-interactive",
            "--provider",
            provider,
            "--channel",
            channel,
        ])
        .status()
        .expect("failed to spawn the cyrene binary for onboarding");
    assert!(status.success(), "onboarding exited with failure: {status}");
}

/// Runs `cyrene doctor` with `$HOME` pointed at `home` and returns its stdout,
/// so the test can assert the diagnostic report (R23.5).
fn run_doctor(home: &Path) -> String {
    let output = cyrene_command(home)
        .arg("doctor")
        .output()
        .expect("failed to spawn the cyrene binary for doctor");
    assert!(output.status.success(), "doctor exited with failure");
    String::from_utf8(output.stdout).expect("doctor stdout was not valid UTF-8")
}

/// Builds a `Command` for the Cargo-built `cyrene` binary with a throwaway home
/// directory, so onboarding writes into the test's temp dir rather than the
/// real user profile. Sets `CYRENE_HOME` which the config crate checks first,
/// plus `HOME`/`USERPROFILE` as fallbacks for any code that still consults
/// `dirs::home_dir()` directly.
fn cyrene_command(home: &Path) -> Command {
    let cyrene_dir = home.join(".cyrene");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cyrene"));
    cmd.current_dir(home)
        .env("CYRENE_HOME", &cyrene_dir)
        .env("HOME", home)
        .env("USERPROFILE", home);
    cmd
}

/// The on-disk paths the onboarded runtime uses, all under `~/.cyrene`.
struct OnboardedPaths {
    config: PathBuf,
    db: PathBuf,
    key: PathBuf,
}

impl OnboardedPaths {
    fn under(home: &Path) -> Self {
        let cyrene_dir = home.join(".cyrene");
        Self {
            config: cyrene_dir.join("config.toml"),
            db: cyrene_dir.join("cyrene.db"),
            key: InstallKey::default_path_in(&cyrene_dir),
        }
    }
}

// ─── Agent_Loop wiring driven by the daemon's request handler ────────────────

/// A planner that turns any CLI request into a single low-risk file-edit step —
/// the kind of step the default autonomy policy auto-approves, so a basic
/// request runs end to end without a human in the loop (R23.4).
struct CliPlanner;
impl Planner for CliPlanner {
    fn plan(&self, session: &Session, _request: &str) -> Result<Plan, String> {
        Ok(Plan::new(
            session.id,
            vec![Step::new(0, StepKind::FileEdit, Risk::Low)
                .with_tool(ToolCall::new("fs.write", json!({ "path": "report.txt" })))],
        ))
    }
}

/// An executor that "writes" a small report file, so the loop has real file
/// bytes to capture in the checkpoint (R4.1).
struct ReportExecutor;
impl Executor for ReportExecutor {
    fn execute(&mut self, step: &Step) -> Result<StepOutput, String> {
        Ok(StepOutput {
            result: format!("wrote the status report (step {})", step.seq),
            files: vec![("report.txt".to_owned(), b"cyrene status report".to_vec())],
        })
    }
}

/// A responder that approves — never exercised by the low-risk plan above, but
/// required to satisfy the loop's human-in-the-loop boundary.
struct AutoApprove;
impl ApprovalResponder for AutoApprove {
    fn respond(&mut self, _request: &ApprovalRequest) -> ApprovalResponse {
        ApprovalResponse::Approve
    }
}

/// What one processed turn recorded, surfaced back to the test thread.
#[derive(Clone)]
struct TurnRecord {
    session_id: SessionId,
    receipts: u64,
    checkpoints: u64,
    response: String,
}

/// The owned safety/audit substrates the handler borrows to build an
/// [`AgentLoop`] per request. Bundled behind one mutex because the ledger and
/// state store each own a (Send, !Sync) SQLite connection.
struct LoopState {
    scanner: InjectionScanner,
    policy: AutonomyPolicy,
    sandbox: Sandbox,
    gate: ApprovalGate,
    ledger: Ledger,
    state: StateStore,
    session: Session,
}

/// A daemon [`RequestHandler`] that runs one Agent_Loop turn per inbound
/// request and records the outcome for the test to assert on.
struct AgentHandler {
    inner: Mutex<LoopState>,
    record: Arc<Mutex<Vec<TurnRecord>>>,
}

impl RequestHandler for AgentHandler {
    fn handle(&self, request: InboundRequest) -> String {
        let mut guard = self.inner.lock().expect("loop state poisoned");
        // Destructure for disjoint field borrows: the loop takes shared borrows
        // of most substrates and a mutable borrow of the approval gate.
        let LoopState {
            scanner,
            policy,
            sandbox,
            gate,
            ledger,
            state,
            session,
        } = &mut *guard;

        let planner = CliPlanner;
        let mut executor = ReportExecutor;
        let mut responder = AutoApprove;

        let outcome = {
            let mut agent = AgentLoop::new(
                &*scanner,
                &*policy,
                ShadowExecutionConfig::default(),
                &*sandbox,
                gate,
                &*ledger,
                &*state,
            );
            agent
                .run_turn(
                    &*session,
                    &request.body,
                    ContentSource::UserInput,
                    &planner,
                    &mut executor,
                    &mut responder,
                )
                .expect("agent turn failed")
        };

        self.record
            .lock()
            .expect("record poisoned")
            .push(TurnRecord {
                session_id: session.id,
                receipts: outcome.receipts,
                checkpoints: outcome.checkpoints,
                response: outcome.response.clone(),
            });
        outcome.response
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn scripted_onboard_then_doctor_reports_runtime_ready() {
    let home = tempfile::tempdir().unwrap();
    run_scripted_onboard(home.path(), "ollama", "cli");

    let paths = OnboardedPaths::under(home.path());

    // Onboarding left a config the runtime can load and validate with no
    // further edits — at least one provider and one channel (R23.2, R23.4).
    assert!(paths.config.exists(), "onboarding must write config.toml");
    let config = Config::load_from_path(&paths.config)
        .expect("onboarded config must load and validate without further configuration");
    let provider = config.providers().next().expect("needs ≥1 provider");
    let channel = config.channels().next().expect("needs ≥1 channel");
    assert_eq!(provider.type_name, "ollama");
    assert_eq!(channel.type_name, "cli");

    // The diagnostic command reports the status of the configured
    // providers/channels and subsystems (R23.5).
    let report = run_doctor(home.path());
    assert!(
        report.contains("Config file found"),
        "doctor should find the onboarded config; got:\n{report}"
    );
    assert!(
        report.contains("Configured providers:"),
        "doctor should report provider status; got:\n{report}"
    );
    assert!(
        report.contains("Configured channels:"),
        "doctor should report channel status; got:\n{report}"
    );
    assert!(
        report.contains("Doctor check complete."),
        "doctor should complete its report; got:\n{report}"
    );
}

#[tokio::test]
async fn onboarded_runtime_processes_cli_request_recording_receipt_and_checkpoint() {
    // ── 1. Scripted install + onboard via the real binary. ──
    let home = tempfile::tempdir().unwrap();
    run_scripted_onboard(home.path(), "ollama", "cli");
    let paths = OnboardedPaths::under(home.path());

    let config = Config::load_from_path(&paths.config).expect("onboarded config loads");
    let provider_type = config
        .providers()
        .next()
        .expect("onboarding configured a provider")
        .type_name
        .to_owned();
    let channel_type = config
        .channels()
        .next()
        .expect("onboarding configured a channel")
        .type_name
        .to_owned();
    assert_eq!(provider_type, "ollama");
    assert_eq!(channel_type, "cli");

    // ── 2. Wire the Agent_Loop from the onboarded config + the persisted
    //       ledger/state at the paths `doctor` points at. ──
    let workspace = tempfile::tempdir().unwrap();
    let loop_state = LoopState {
        scanner: InjectionScanner::new(),
        // The policy comes straight from the onboarded (secure-by-default)
        // autonomy config, so the request is gated exactly as configured.
        policy: AutonomyPolicy::new(config.autonomy.clone()),
        sandbox: Sandbox::new(workspace.path(), SandboxBackend::CopyOnWrite).unwrap(),
        gate: ApprovalGate::new(),
        ledger: Ledger::open(&paths.db, &paths.key).expect("open onboarded ledger"),
        state: StateStore::open(&paths.db).expect("open onboarded state store"),
        // The session originates on the onboarded channel (replies route here).
        session: Session::new(
            UserId::new("cli-user"),
            ChannelOrigin::new(channel_type.clone()),
            Budget::unlimited(),
        ),
    };
    let session_id = loop_state.session.id;

    let record = Arc::new(Mutex::new(Vec::<TurnRecord>::new()));
    let handler = AgentHandler {
        inner: Mutex::new(loop_state),
        record: Arc::clone(&record),
    };

    // ── 3. Dispatch a CLI request through the real daemon cold path. ──
    let daemon = Daemon::new(handler, 16);
    let handle = daemon.handle();
    let join = tokio::spawn(daemon.run());

    let inbound = InboundRequest::new(session_id, channel_type, "please write a status report");

    // R1.3: receipt → enqueue is an O(1) bounded-channel send, far inside the
    // 100ms cold-path budget. (Generous margin so this isn't flaky on CI.)
    let start = Instant::now();
    handle.dispatch(inbound).await.unwrap();
    let enqueue = start.elapsed();
    assert!(
        enqueue < Duration::from_millis(100),
        "inbound dispatch {enqueue:?} exceeded the 100ms cold-path budget (R1.3)"
    );

    // Drop the handle so the daemon drains the queued request and exits.
    drop(handle);
    let processed = join.await.unwrap();
    assert_eq!(processed, 1, "the daemon must process exactly one request");

    // ── 4. The turn recorded a receipt and a checkpoint. ──
    let records = record.lock().unwrap();
    assert_eq!(records.len(), 1, "exactly one turn was processed");
    let turn = &records[0];
    assert_eq!(turn.session_id, session_id);
    assert!(
        turn.receipts >= 2,
        "expected at least a plan + step-executed receipt, got {}",
        turn.receipts
    );
    assert_eq!(
        turn.checkpoints, 1,
        "the executed step recorded one checkpoint"
    );
    assert!(
        !turn.response.is_empty(),
        "the runtime returned a response to the request"
    );

    // ── 5. The receipt/checkpoint were persisted to the onboarded DB and the
    //       ledger chain verifies (R5.1/5.2, R4.1, R23.4). ──
    assert!(
        paths.db.exists(),
        "the request must create the database `doctor` reports"
    );

    let ledger = Ledger::open(&paths.db, &paths.key).expect("reopen persisted ledger");
    assert!(
        ledger.verify().unwrap().is_valid(),
        "the persisted receipt chain must verify"
    );
    let receipts = ledger.receipts_for_session(session_id).unwrap();
    assert!(
        receipts.len() >= 2,
        "the session's receipts must be persisted, got {}",
        receipts.len()
    );

    let state = StateStore::open(&paths.db).expect("reopen persisted state store");
    let history = state.history(session_id).unwrap();
    assert_eq!(
        history.len(),
        1,
        "the session's checkpoint must be persisted"
    );
}
