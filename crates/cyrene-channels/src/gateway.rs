//! The Channel_Gateway: fans every registered channel into one Agent_Loop
//! (R7.1), replies on the originating channel (R7.4), preserves session context
//! across channels (R7.5), and degrades gracefully when a channel drops (R7.6).
//!
//! The gateway is transport-agnostic: it holds `Arc<dyn Channel>` instances
//! keyed by [`ChannelId`] and an [`crate::SessionStore`] for continuity. A
//! turn-handler closure stands in for the Agent_Loop so the gateway is testable
//! in isolation; the runtime injects the real loop.

use std::collections::BTreeMap;
use std::sync::Arc;

use cyrene_core::{
    Channel, ChannelError, ChannelHealth, ChannelId, InboundMessage, OutboundMessage, SessionId,
};

use crate::session_store::SessionStore;

/// A record of a channel that became unavailable, for ledger logging (R7.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelDrop {
    /// The channel that dropped.
    pub channel: ChannelId,
    /// Why it became unavailable.
    pub reason: String,
}

/// The result of one gateway poll-and-dispatch cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleReport {
    /// How many messages were handled and replied to this cycle.
    pub handled: usize,
    /// Channels found unavailable this cycle (logged + skipped, R7.6).
    pub drops: Vec<ChannelDrop>,
}

/// Fans multiple channels into one loop and routes replies back to origin.
pub struct ChannelGateway {
    channels: BTreeMap<ChannelId, Arc<dyn Channel>>,
    sessions: SessionStore,
}

impl Default for ChannelGateway {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelGateway {
    /// Creates an empty gateway.
    #[must_use]
    pub fn new() -> Self {
        Self {
            channels: BTreeMap::new(),
            sessions: SessionStore::new(),
        }
    }

    /// Registers a channel. A new channel implementing [`Channel`] routes with
    /// no core change (R7.7). Re-registering an id replaces the prior channel.
    pub fn register(&mut self, channel: Arc<dyn Channel>) {
        self.channels.insert(channel.id(), channel);
    }

    /// The number of registered channels.
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Borrows the shared session store (cross-channel continuity, R7.5).
    #[must_use]
    pub fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    /// Runs one poll-dispatch-reply cycle across every healthy channel.
    ///
    /// For each channel: if unavailable, it is recorded as a drop and skipped
    /// while the rest continue serving (R7.6). Otherwise its pending message (if
    /// any) is resolved to a shared session (R7.5), handed to `handle` (the
    /// Agent_Loop, R7.1), and the response is sent back on the **same** channel
    /// the request arrived on (R7.4).
    ///
    /// `handle` receives the resolved [`SessionId`] and the inbound message and
    /// returns the response text.
    pub async fn run_cycle<F>(&self, mut handle: F) -> CycleReport
    where
        F: FnMut(SessionId, &InboundMessage) -> String,
    {
        let mut handled = 0;
        let mut drops = Vec::new();

        for (id, channel) in &self.channels {
            // Graceful degradation: skip + record unavailable channels (R7.6).
            if let ChannelHealth::Unavailable { reason } = channel.health().await {
                drops.push(ChannelDrop {
                    channel: id.clone(),
                    reason,
                });
                continue;
            }

            match channel.poll().await {
                Ok(Some(inbound)) => {
                    let session = self.sessions.session_for(&inbound);
                    let response = handle(session, &inbound);
                    let reply = OutboundMessage::reply_to(&inbound, response);
                    // Reply on the originating channel (R7.4).
                    if let Err(err) = channel.send(reply).await {
                        drops.push(ChannelDrop {
                            channel: id.clone(),
                            reason: format!("send failed: {err}"),
                        });
                    } else {
                        handled += 1;
                    }
                }
                Ok(None) => { /* nothing pending on this channel */ }
                Err(err) => {
                    // A transport error degrades this channel but not the rest.
                    drops.push(ChannelDrop {
                        channel: id.clone(),
                        reason: drop_reason(&err),
                    });
                }
            }
        }

        CycleReport { handled, drops }
    }
}

/// Renders a channel error as a drop reason string.
fn drop_reason(err: &ChannelError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeChannel;
    use cyrene_core::UserId;

    #[tokio::test]
    async fn fans_in_multiple_channels_to_one_handler() {
        let mut gw = ChannelGateway::new();
        let cli = Arc::new(FakeChannel::new("cli"));
        let tg = Arc::new(FakeChannel::new("telegram"));
        cli.push_inbound(InboundMessage::new("cli", UserId::new("alice"), "from cli"));
        tg.push_inbound(InboundMessage::new(
            "telegram",
            UserId::new("bob"),
            "from tg",
        ));
        gw.register(cli.clone());
        gw.register(tg.clone());

        let report = gw.run_cycle(|_s, msg| format!("ack:{}", msg.text)).await;
        assert_eq!(report.handled, 2);
        assert!(report.drops.is_empty());

        // Each reply landed on its originating channel (R7.4).
        assert_eq!(cli.sent_texts(), vec!["ack:from cli".to_owned()]);
        assert_eq!(tg.sent_texts(), vec!["ack:from tg".to_owned()]);
    }

    #[tokio::test]
    async fn unavailable_channel_is_logged_and_others_keep_serving() {
        let mut gw = ChannelGateway::new();
        let good = Arc::new(FakeChannel::new("cli"));
        let bad = Arc::new(FakeChannel::new("slack"));
        good.push_inbound(InboundMessage::new("cli", UserId::new("alice"), "hi"));
        bad.set_unavailable("disconnected");
        gw.register(good.clone());
        gw.register(bad.clone());

        let report = gw.run_cycle(|_s, _m| "ok".to_owned()).await;

        // The good channel still served (R7.6)...
        assert_eq!(report.handled, 1);
        assert_eq!(good.sent_texts(), vec!["ok".to_owned()]);
        // ...and the bad channel was recorded as a drop.
        assert_eq!(report.drops.len(), 1);
        assert_eq!(report.drops[0].channel, ChannelId::new("slack"));
        assert!(report.drops[0].reason.contains("disconnected"));
    }

    #[tokio::test]
    async fn reply_rides_origin_thread_for_continuity() {
        let mut gw = ChannelGateway::new();
        let slack = Arc::new(FakeChannel::new("slack"));
        slack.push_inbound(
            InboundMessage::new("slack", UserId::new("alice"), "q").with_thread("c-7"),
        );
        gw.register(slack.clone());

        gw.run_cycle(|_s, _m| "a".to_owned()).await;
        // The reply carries the originating thread.
        assert_eq!(slack.sent_threads(), vec![Some("c-7".to_owned())]);
    }

    #[tokio::test]
    async fn same_user_keeps_session_across_channels() {
        let mut gw = ChannelGateway::new();
        let tg = Arc::new(FakeChannel::new("telegram"));
        let slack = Arc::new(FakeChannel::new("slack"));
        tg.push_inbound(
            InboundMessage::new("telegram", UserId::new("alice"), "1").with_thread("shared"),
        );
        slack.push_inbound(
            InboundMessage::new("slack", UserId::new("alice"), "2").with_thread("shared"),
        );
        gw.register(tg.clone());
        gw.register(slack.clone());

        let mut seen = Vec::new();
        gw.run_cycle(|s, _m| {
            seen.push(s);
            "ok".to_owned()
        })
        .await;

        // Both messages resolved to the same session id (R7.5).
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0], seen[1]);
    }

    #[tokio::test]
    async fn send_failure_degrades_only_that_channel() {
        let mut gw = ChannelGateway::new();
        let ch = Arc::new(FakeChannel::new("cli"));
        ch.push_inbound(InboundMessage::new("cli", UserId::new("alice"), "hi"));
        ch.fail_send("smtp down");
        gw.register(ch.clone());

        let report = gw.run_cycle(|_s, _m| "ok".to_owned()).await;
        assert_eq!(report.handled, 0);
        assert_eq!(report.drops.len(), 1);
        assert!(report.drops[0].reason.contains("smtp down"));
    }
}
