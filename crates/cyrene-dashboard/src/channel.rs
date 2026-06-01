//! The Dashboard channel binding (R26.3).
//!
//! A message submitted through the dashboard enters the **same** Agent_Loop as
//! any other channel by implementing the [`Channel`] trait. Inbound messages
//! are pushed by the WebSocket handler; outbound responses are buffered for the
//! socket to stream back to the browser.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;

use cyrene_core::{
    Channel, ChannelError, ChannelHealth, ChannelId, InboundMessage, OutboundMessage,
};

/// A [`Channel`] backed by the local web dashboard.
#[derive(Debug, Default)]
pub struct DashboardChannel {
    inbound: Mutex<VecDeque<InboundMessage>>,
    outbound: Mutex<Vec<OutboundMessage>>,
}

impl DashboardChannel {
    /// Creates an empty dashboard channel.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Submits a message typed in the dashboard chat (called by the WS handler).
    pub fn submit(&self, msg: InboundMessage) {
        self.inbound.lock().unwrap().push_back(msg);
    }

    /// Drains the buffered responses for the socket to stream to the browser.
    #[must_use]
    pub fn drain_responses(&self) -> Vec<OutboundMessage> {
        std::mem::take(&mut *self.outbound.lock().unwrap())
    }
}

#[async_trait]
impl Channel for DashboardChannel {
    fn id(&self) -> ChannelId {
        ChannelId::new("dashboard")
    }

    async fn poll(&self) -> Result<Option<InboundMessage>, ChannelError> {
        Ok(self.inbound.lock().unwrap().pop_front())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<(), ChannelError> {
        self.outbound.lock().unwrap().push(msg);
        Ok(())
    }

    async fn health(&self) -> ChannelHealth {
        ChannelHealth::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrene_core::UserId;

    #[tokio::test]
    async fn submitted_message_is_polled() {
        let ch = DashboardChannel::new();
        ch.submit(InboundMessage::new(
            "dashboard",
            UserId::new("alice"),
            "status?",
        ));
        let got = ch.poll().await.unwrap().unwrap();
        assert_eq!(got.text, "status?");
        assert_eq!(ch.id(), ChannelId::new("dashboard"));
    }

    #[tokio::test]
    async fn responses_are_buffered_and_drained() {
        let ch = DashboardChannel::new();
        ch.send(OutboundMessage::new(UserId::new("alice"), "all good"))
            .await
            .unwrap();
        let out = ch.drain_responses();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "all good");
        // Draining again yields nothing.
        assert!(ch.drain_responses().is_empty());
    }

    #[tokio::test]
    async fn dashboard_channel_is_healthy() {
        let ch = DashboardChannel::new();
        assert!(matches!(ch.health().await, ChannelHealth::Healthy));
    }
}
