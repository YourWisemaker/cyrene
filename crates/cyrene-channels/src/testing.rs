//! A `FakeChannel` test double for exercising the gateway without real
//! transports. Available to integration tests via `cyrene_channels::testing`.

use std::sync::Mutex;

use async_trait::async_trait;

use cyrene_core::{
    Channel, ChannelError, ChannelHealth, ChannelId, InboundMessage, OutboundMessage,
};

/// An in-memory channel: inbound messages are queued by the test, and outbound
/// messages are captured for assertions. Health and failures are controllable.
#[derive(Debug)]
pub struct FakeChannel {
    id: ChannelId,
    inbound: Mutex<Vec<InboundMessage>>,
    sent: Mutex<Vec<OutboundMessage>>,
    health: Mutex<ChannelHealth>,
    send_error: Mutex<Option<String>>,
}

impl FakeChannel {
    /// Creates a healthy fake channel with the given id.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self {
            id: ChannelId::new(id),
            inbound: Mutex::new(Vec::new()),
            sent: Mutex::new(Vec::new()),
            health: Mutex::new(ChannelHealth::Healthy),
            send_error: Mutex::new(None),
        }
    }

    /// Queues an inbound message to be returned by the next `poll`.
    pub fn push_inbound(&self, msg: InboundMessage) {
        self.inbound.lock().unwrap().push(msg);
    }

    /// Marks the channel unavailable with a reason.
    pub fn set_unavailable(&self, reason: &str) {
        *self.health.lock().unwrap() = ChannelHealth::Unavailable {
            reason: reason.to_owned(),
        };
    }

    /// Makes the next `send` fail with a transport error.
    pub fn fail_send(&self, reason: &str) {
        *self.send_error.lock().unwrap() = Some(reason.to_owned());
    }

    /// Returns the texts of all sent messages, in order.
    #[must_use]
    pub fn sent_texts(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.text.clone())
            .collect()
    }

    /// Returns the thread keys of all sent messages, in order.
    #[must_use]
    pub fn sent_threads(&self) -> Vec<Option<String>> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.thread.clone())
            .collect()
    }
}

#[async_trait]
impl Channel for FakeChannel {
    fn id(&self) -> ChannelId {
        self.id.clone()
    }

    async fn poll(&self) -> Result<Option<InboundMessage>, ChannelError> {
        Ok(self.inbound.lock().unwrap().pop())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<(), ChannelError> {
        if let Some(reason) = self.send_error.lock().unwrap().take() {
            return Err(ChannelError::Transport(reason));
        }
        self.sent.lock().unwrap().push(msg);
        Ok(())
    }

    async fn health(&self) -> ChannelHealth {
        self.health.lock().unwrap().clone()
    }
}
