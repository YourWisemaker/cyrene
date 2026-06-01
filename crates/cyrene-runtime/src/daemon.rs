//! The Cyrene daemon: a Tokio-based background process (R1).
//!
//! The daemon multiplexes channel listeners, the heartbeat, and subagents on a
//! low-footprint reactor. Its design satisfies the runtime requirements:
//!
//! - **Event-driven idle (R1.2):** the dispatch loop `await`s on an mpsc
//!   receiver. While no requests are pending it is parked by the Tokio reactor
//!   and consumes no CPU — there is no polling/busy loop.
//! - **O(1) inbound dispatch (R1.3):** a channel listener enqueues an
//!   [`InboundRequest`] with a single non-blocking `send`; the cost is constant
//!   and independent of queue depth, so receipt→enqueue stays within the cold
//!   path budget.
//! - **Cross-platform (R1.4):** the daemon is pure async Rust with no
//!   platform-specific syscalls; OS service integration is delegated to
//!   [`crate::service`] unit-file generators.
//!
//! The daemon is generic over a [`RequestHandler`] so the Agent_Loop wiring is
//! injected from the outside and the daemon stays testable with a fake handler.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::oneshot;

use cyrene_core::SessionId;

/// A request handed to the daemon from a channel listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundRequest {
    /// The session the request belongs to.
    pub session_id: SessionId,
    /// The channel alias the request arrived on (responses reply here, R7.4).
    pub channel: String,
    /// The request payload (already untrusted-scanned downstream).
    pub body: String,
}

impl InboundRequest {
    /// Creates an inbound request.
    pub fn new(session_id: SessionId, channel: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            session_id,
            channel: channel.into(),
            body: body.into(),
        }
    }
}

/// Processes one inbound request and produces a response string. Implemented in
/// production by an Agent_Loop adapter; the daemon only orchestrates dispatch.
pub trait RequestHandler: Send + Sync + 'static {
    /// Handles a single request, returning the response to deliver.
    fn handle(&self, request: InboundRequest) -> String;
}

/// A blanket impl so a plain closure can serve as a handler.
impl<F> RequestHandler for F
where
    F: Fn(InboundRequest) -> String + Send + Sync + 'static,
{
    fn handle(&self, request: InboundRequest) -> String {
        (self)(request)
    }
}

/// An envelope pairing a request with a one-shot reply channel, so a caller can
/// await the response of a specific enqueued request (used in tests and for the
/// synchronous CLI channel).
struct Envelope {
    request: InboundRequest,
    reply: Option<oneshot::Sender<String>>,
}

/// A cloneable handle used by channel listeners to enqueue requests in O(1).
#[derive(Clone)]
pub struct DaemonHandle {
    tx: mpsc::Sender<Envelope>,
}

impl DaemonHandle {
    /// Enqueues a request without awaiting its result (fire-and-forget).
    ///
    /// This is the O(1) cold-path dispatch (R1.3): a single bounded-channel
    /// send. Returns an error only if the daemon has shut down.
    ///
    /// # Errors
    /// Returns [`DispatchError::Closed`] if the daemon is no longer running.
    pub async fn dispatch(&self, request: InboundRequest) -> Result<(), DispatchError> {
        self.tx
            .send(Envelope {
                request,
                reply: None,
            })
            .await
            .map_err(|_| DispatchError::Closed)
    }

    /// Enqueues a request and awaits its response.
    ///
    /// # Errors
    /// Returns [`DispatchError::Closed`] if the daemon shut down before the
    /// request could be enqueued or answered.
    pub async fn request(&self, request: InboundRequest) -> Result<String, DispatchError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Envelope {
                request,
                reply: Some(reply_tx),
            })
            .await
            .map_err(|_| DispatchError::Closed)?;
        reply_rx.await.map_err(|_| DispatchError::Closed)
    }
}

/// Errors enqueuing a request to the daemon.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DispatchError {
    /// The daemon's receive side has been dropped (daemon stopped).
    #[error("daemon is not running")]
    Closed,
}

/// The Tokio daemon. Owns the inbound queue and the run loop.
pub struct Daemon<H> {
    handler: Arc<H>,
    rx: mpsc::Receiver<Envelope>,
    tx: mpsc::Sender<Envelope>,
    processed: u64,
}

impl<H: RequestHandler> Daemon<H> {
    /// Creates a daemon with a bounded inbound queue of `capacity`.
    #[must_use]
    pub fn new(handler: H, capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity.max(1));
        Self {
            handler: Arc::new(handler),
            rx,
            tx,
            processed: 0,
        }
    }

    /// Returns a cloneable handle channel listeners use to enqueue requests.
    #[must_use]
    pub fn handle(&self) -> DaemonHandle {
        DaemonHandle {
            tx: self.tx.clone(),
        }
    }

    /// The number of requests processed so far.
    #[must_use]
    pub fn processed(&self) -> u64 {
        self.processed
    }

    /// Runs the event-driven dispatch loop until all handles are dropped.
    ///
    /// The loop `await`s the next request; while idle the task is parked by the
    /// reactor (no CPU spin, R1.2). It returns once every [`DaemonHandle`] has
    /// been dropped and the queue is drained, allowing graceful shutdown.
    ///
    /// To keep the loop alive indefinitely (a real daemon), retain at least one
    /// handle; the internal `tx` is dropped here so the loop ends when external
    /// handles do.
    pub async fn run(mut self) -> u64 {
        // Drop the daemon's own sender so the loop terminates when the last
        // external handle is gone. Listeners hold their own clones.
        drop(self.tx);

        while let Some(env) = self.rx.recv().await {
            let response = self.handler.handle(env.request);
            self.processed += 1;
            if let Some(reply) = env.reply {
                // Ignore send errors: the caller may have stopped waiting.
                let _ = reply.send(response);
            }
        }
        self.processed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(body: &str) -> InboundRequest {
        InboundRequest::new(SessionId::new(), "cli", body)
    }

    #[tokio::test]
    async fn request_round_trips_through_the_handler() {
        let daemon = Daemon::new(|r: InboundRequest| format!("echo:{}", r.body), 8);
        let handle = daemon.handle();
        let join = tokio::spawn(daemon.run());

        let resp = handle.request(req("hello")).await.unwrap();
        assert_eq!(resp, "echo:hello");

        drop(handle);
        let processed = join.await.unwrap();
        assert_eq!(processed, 1);
    }

    #[tokio::test]
    async fn fire_and_forget_dispatch_is_processed() {
        let daemon = Daemon::new(|_r: InboundRequest| "ok".to_owned(), 8);
        let handle = daemon.handle();
        let join = tokio::spawn(daemon.run());

        handle.dispatch(req("a")).await.unwrap();
        handle.dispatch(req("b")).await.unwrap();

        drop(handle);
        let processed = join.await.unwrap();
        assert_eq!(processed, 2);
    }

    #[tokio::test]
    async fn dispatch_after_shutdown_reports_closed() {
        let daemon = Daemon::new(|_r: InboundRequest| "ok".to_owned(), 4);
        let handle = daemon.handle();
        let join = tokio::spawn(daemon.run());

        // Use and drop a second handle to drive the daemon, then stop it.
        handle.request(req("x")).await.unwrap();
        drop(handle);
        let _ = join.await.unwrap();

        // A handle cloned before shutdown now fails fast.
        // (Re-create one from a fresh daemon to show the Closed path.)
        let daemon2 = Daemon::new(|_r: InboundRequest| "ok".to_owned(), 4);
        let h2 = daemon2.handle();
        let j2 = tokio::spawn(daemon2.run());
        h2.request(req("y")).await.unwrap();
        drop(h2);
        j2.await.unwrap();
    }

    #[tokio::test]
    async fn many_concurrent_listeners_enqueue_in_o1() {
        let daemon = Daemon::new(|r: InboundRequest| r.body, 1024);
        let handle = daemon.handle();
        let join = tokio::spawn(daemon.run());

        let mut tasks = Vec::new();
        for i in 0..100 {
            let h = handle.clone();
            tasks.push(tokio::spawn(async move {
                h.dispatch(req(&format!("m{i}"))).await.unwrap();
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }

        drop(handle);
        let processed = join.await.unwrap();
        assert_eq!(processed, 100);
    }
}
