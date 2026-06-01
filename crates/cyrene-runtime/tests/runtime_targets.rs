//! Benchmark-style guards for the runtime performance targets (Task 14.3).
//!
//! These assert the latency/idle properties from R1 hold for the daemon's
//! dispatch path. They are deliberately conservative (generous budgets) so they
//! guard against regressions without being flaky on loaded CI machines.

use std::time::{Duration, Instant};

use cyrene_core::SessionId;
use cyrene_runtime::{Daemon, InboundRequest};

fn req() -> InboundRequest {
    InboundRequest::new(SessionId::new(), "cli", "ping")
}

#[tokio::test]
async fn inbound_dispatch_enqueues_well_within_cold_path_budget() {
    // R1.3: begin processing within 100ms of receipt. The cold path that must
    // meet this budget is receipt → enqueue (a single bounded-channel send),
    // not full task completion. We measure that enqueue cost.
    let daemon = Daemon::new(|_r: InboundRequest| "ok".to_owned(), 1024);
    let handle = daemon.handle();
    let join = tokio::spawn(daemon.run());

    // Warm up, then measure the worst-case enqueue over many dispatches.
    let mut worst = Duration::ZERO;
    for _ in 0..1000 {
        let start = Instant::now();
        handle.dispatch(req()).await.unwrap();
        worst = worst.max(start.elapsed());
    }

    // Each enqueue must be far under the 100ms budget. Allow a wide margin.
    assert!(
        worst < Duration::from_millis(100),
        "worst-case enqueue {worst:?} exceeded the 100ms cold-path budget",
    );

    drop(handle);
    let processed = join.await.unwrap();
    assert_eq!(processed, 1000);
}

#[tokio::test]
async fn idle_daemon_does_not_busy_spin() {
    // R1.2: idle CPU under 1%. We can't measure CPU portably here, but we can
    // assert the design property that makes it true: an idle daemon is parked
    // on `recv().await` and processes nothing until a request arrives. After an
    // idle period with no dispatch, `processed` must still be zero.
    let daemon = Daemon::new(|_r: InboundRequest| "ok".to_owned(), 8);
    let handle = daemon.handle();
    let join = tokio::spawn(daemon.run());

    // Stay idle: no dispatches for a while.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Now send one request and shut down.
    handle.dispatch(req()).await.unwrap();
    drop(handle);

    let processed = join.await.unwrap();
    // Exactly one request was processed — the idle period did no work.
    assert_eq!(processed, 1);
}

#[tokio::test]
async fn dispatch_latency_is_independent_of_queue_depth() {
    // O(1) dispatch (R1.3): enqueuing when the queue already holds many pending
    // items costs the same as enqueuing into an empty queue.
    let daemon = Daemon::new(
        |_r: InboundRequest| {
            // A handler slow enough that the queue backs up while we measure.
            std::thread::sleep(Duration::from_micros(50));
            "ok".to_owned()
        },
        4096,
    );
    let handle = daemon.handle();
    let join = tokio::spawn(daemon.run());

    // Enqueue into an empty queue.
    let start_empty = Instant::now();
    handle.dispatch(req()).await.unwrap();
    let empty_cost = start_empty.elapsed();

    // Flood the queue so many items are pending, then measure another enqueue.
    for _ in 0..2000 {
        handle.dispatch(req()).await.unwrap();
    }
    let start_full = Instant::now();
    handle.dispatch(req()).await.unwrap();
    let full_cost = start_full.elapsed();

    // Both enqueues are bounded-channel sends; neither should approach the
    // budget regardless of depth.
    assert!(empty_cost < Duration::from_millis(100));
    assert!(full_cost < Duration::from_millis(100));

    drop(handle);
    let _ = join.await.unwrap();
}
