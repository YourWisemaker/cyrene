//! `cyrene-channels`: the Channel_Gateway and built-in messaging channels (R7).
//!
//! Cyrene reaches the user wherever they already are. This crate provides:
//!
//! - [`ChannelGateway`] — fans every registered [`cyrene_core::Channel`] into
//!   one Agent_Loop (R7.1), replies on the originating channel (R7.4),
//!   preserves session context across channels via a shared [`SessionStore`]
//!   (R7.5), and degrades gracefully when a channel drops (R7.6).
//! - Built-in channels: [`CliChannel`] plus a generic [`RemoteChannel`] over a
//!   [`Transport`] for Telegram, Slack, Discord, WhatsApp, email/Gmail, Signal,
//!   and Matrix (R7.2, R7.3).
//! - [`InboundAuth`] — DM pairing + allowlist authentication for inbound
//!   senders (R7.2, R22.5).

mod auth;
mod channels;
mod gateway;
mod session_store;

pub mod testing;

pub use auth::{AuthError, InboundAuth};
pub use channels::{CliChannel, Delivery, RawInbound, RemoteChannel, RemoteKind, Transport};
pub use gateway::{ChannelDrop, ChannelGateway, CycleReport};
pub use session_store::{ConversationKey, SessionStore};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-channels"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
