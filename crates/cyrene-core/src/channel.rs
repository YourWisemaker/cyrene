//! The [`Channel`] trait and its message/health types.
//!
//! A [`Channel`] is an external messaging surface (CLI, Telegram, Slack, …).
//! The Channel_Gateway (task 15) fans every registered channel into one
//! Agent_Loop (R7.1), replies on the channel a request arrived on (R7.4), and
//! keeps serving the rest when one channel drops (R7.6). New channels that
//! implement this trait route with no core change (R7.7). A channel is keyed by
//! a [`ChannelId`], reusing the [`ChannelOrigin`](crate::ChannelOrigin) string
//! newtype so a message's origin and its channel share one identifier space.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{Recoverability, Recoverable};
use crate::ids::{ChannelOrigin, UserId};

/// Identifies a registered [`Channel`].
///
/// This is an alias of [`ChannelOrigin`] so an [`InboundMessage::origin`] can be
/// compared directly against the channel that produced it when routing replies.
pub type ChannelId = ChannelOrigin;

/// The kind of media an [`Attachment`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    /// A voice note or other audio clip, transcribed to text via a
    /// speech-to-text tool before the Agent_Loop sees it.
    Audio,
    /// A still image.
    Image,
    /// A video clip.
    Video,
    /// A document or other file.
    File,
}

/// A media attachment that arrived alongside an [`InboundMessage`].
///
/// Channels that receive voice notes (Telegram, WhatsApp, …) surface them here
/// with [`AttachmentKind::Audio`]; the Channel_Gateway transcribes audio to
/// text with a speech-to-text tool (e.g. `stt.transcribe`, backed by ElevenLabs
/// or Whisper) and folds the transcript into [`InboundMessage::text`] before
/// the Agent_Loop runs, so downstream code stays text-only (R7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// What kind of media this is.
    pub kind: AttachmentKind,
    /// A URL or channel-native handle the runtime fetches the bytes from.
    pub url: String,
    /// The MIME type the channel reported, if any (e.g. `"audio/ogg"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl Attachment {
    /// Creates an attachment of `kind` referencing `url`.
    pub fn new(kind: AttachmentKind, url: impl Into<String>) -> Self {
        Self {
            kind,
            url: url.into(),
            mime_type: None,
        }
    }

    /// Sets the reported MIME type.
    #[must_use]
    pub fn with_mime(mut self, mime: impl Into<String>) -> Self {
        self.mime_type = Some(mime.into());
        self
    }

    /// Whether this attachment is audio (a voice note) needing transcription.
    #[must_use]
    pub fn is_audio(&self) -> bool {
        matches!(self.kind, AttachmentKind::Audio)
    }
}

/// A message received from a [`Channel`], bound for the Agent_Loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundMessage {
    /// The channel this message arrived on (replies default here, R7.4).
    pub origin: ChannelId,
    /// The user who sent the message.
    pub user_id: UserId,
    /// The message text.
    pub text: String,
    /// Opaque channel-native conversation/thread key, used to preserve session
    /// context across channels (R7.5). `None` when the channel is not threaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// Media attachments (voice notes, images, files) that arrived with the
    /// message. Audio attachments are transcribed into [`InboundMessage::text`]
    /// by the gateway before the Agent_Loop runs (R7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

impl InboundMessage {
    /// Creates an inbound message with no thread key.
    pub fn new(origin: impl Into<ChannelId>, user_id: UserId, text: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
            user_id,
            text: text.into(),
            thread: None,
            attachments: Vec::new(),
        }
    }

    /// Attaches a channel-native thread/conversation key.
    #[must_use]
    pub fn with_thread(mut self, thread: impl Into<String>) -> Self {
        self.thread = Some(thread.into());
        self
    }

    /// Attaches a single media attachment (e.g. a voice note).
    #[must_use]
    pub fn with_attachment(mut self, attachment: Attachment) -> Self {
        self.attachments.push(attachment);
        self
    }

    /// Replaces the message's attachments.
    #[must_use]
    pub fn with_attachments(mut self, attachments: Vec<Attachment>) -> Self {
        self.attachments = attachments;
        self
    }

    /// Returns the first audio attachment (voice note) needing transcription,
    /// if any.
    #[must_use]
    pub fn voice_note(&self) -> Option<&Attachment> {
        self.attachments.iter().find(|a| a.is_audio())
    }
}

/// A message the Agent_Loop sends out through a [`Channel`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundMessage {
    /// The user to deliver the message to.
    pub user_id: UserId,
    /// The message text.
    pub text: String,
    /// The thread/conversation key to reply within, if any (R7.4/7.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
}

impl OutboundMessage {
    /// Creates an outbound message with no thread key.
    pub fn new(user_id: UserId, text: impl Into<String>) -> Self {
        Self {
            user_id,
            text: text.into(),
            thread: None,
        }
    }

    /// Creates an outbound reply to an [`InboundMessage`], carrying its thread
    /// key and recipient so the gateway can reply on the originating channel.
    #[must_use]
    pub fn reply_to(inbound: &InboundMessage, text: impl Into<String>) -> Self {
        Self {
            user_id: inbound.user_id.clone(),
            text: text.into(),
            thread: inbound.thread.clone(),
        }
    }

    /// Sets the thread/conversation key to reply within.
    #[must_use]
    pub fn with_thread(mut self, thread: impl Into<String>) -> Self {
        self.thread = Some(thread.into());
        self
    }
}

/// The health of a [`Channel`] connection.
///
/// The gateway uses this to decide whether to keep routing to a channel or to
/// degrade gracefully and log the drop while serving the rest (R7.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelHealth {
    /// The channel is connected and serving traffic.
    Healthy,
    /// The channel is reachable but impaired (e.g. rate limited, reconnecting).
    Degraded {
        /// A human-readable reason for the degradation.
        reason: String,
    },
    /// The channel is unavailable; the gateway should stop routing to it.
    Unavailable {
        /// A human-readable reason the channel is down.
        reason: String,
    },
}

impl ChannelHealth {
    /// Returns `true` when the channel can currently serve traffic.
    #[must_use]
    pub fn is_available(&self) -> bool {
        !matches!(self, Self::Unavailable { .. })
    }
}

/// Errors a [`Channel`] implementation can return.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// The channel is not connected or its transport is down.
    #[error("channel unavailable: {0}")]
    Unavailable(String),

    /// Sending or receiving failed at the transport layer.
    #[error("channel transport error: {0}")]
    Transport(String),

    /// The channel rejected the message as invalid (e.g. too long, bad target).
    #[error("invalid channel message: {0}")]
    InvalidMessage(String),

    /// A message payload failed to (de)serialize.
    #[error("channel serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl Recoverable for ChannelError {
    fn recoverability(&self) -> Recoverability {
        match self {
            // Transient transport/availability issues are worth retrying; the
            // gateway degrades gracefully if they persist (R7.6).
            Self::Unavailable(_) | Self::Transport(_) => Recoverability::Retry,
            // A malformed message will not be fixed by retrying.
            Self::InvalidMessage(_) | Self::Serialization(_) => Recoverability::Halt,
        }
    }
}

/// An external messaging surface connected to the Agent_Loop.
///
/// Registered in the Plugin_Registry and routed by the Channel_Gateway; new
/// implementations need no core change (R2.1, R7.7).
#[async_trait]
pub trait Channel: Send + Sync {
    /// Returns this channel's stable identifier (e.g. `"cli"`, `"telegram"`).
    fn id(&self) -> ChannelId;

    /// Polls for the next inbound message, returning `None` when none is
    /// currently available.
    ///
    /// # Errors
    /// Returns a [`ChannelError`] if the channel's transport fails.
    async fn poll(&self) -> Result<Option<InboundMessage>, ChannelError>;

    /// Sends an outbound message through this channel.
    ///
    /// # Errors
    /// Returns a [`ChannelError`] if delivery fails.
    async fn send(&self, msg: OutboundMessage) -> Result<(), ChannelError>;

    /// Reports the current health of the channel connection.
    async fn health(&self) -> ChannelHealth;
}

#[cfg(test)]
mod tests {
    use super::{ChannelHealth, InboundMessage, OutboundMessage};
    use crate::ids::{ChannelOrigin, UserId};

    #[test]
    fn inbound_message_new_has_no_thread() {
        let msg = InboundMessage::new("cli", UserId::new("alice"), "hello");
        assert_eq!(msg.origin, ChannelOrigin::new("cli"));
        assert_eq!(msg.user_id, UserId::new("alice"));
        assert_eq!(msg.text, "hello");
        assert!(msg.thread.is_none());
    }

    #[test]
    fn inbound_message_with_thread_sets_key() {
        let msg = InboundMessage::new("telegram", UserId::new("alice"), "hi").with_thread("t-42");
        assert_eq!(msg.thread.as_deref(), Some("t-42"));
    }

    #[test]
    fn outbound_reply_to_carries_user_and_thread() {
        let inbound = InboundMessage::new("slack", UserId::new("bob"), "ping").with_thread("c-7");
        let reply = OutboundMessage::reply_to(&inbound, "pong");
        assert_eq!(reply.user_id, UserId::new("bob"));
        assert_eq!(reply.text, "pong");
        // Reply rides the originating thread so it lands on the same channel (R7.4).
        assert_eq!(reply.thread.as_deref(), Some("c-7"));
    }

    #[test]
    fn channel_health_availability() {
        assert!(ChannelHealth::Healthy.is_available());
        assert!(ChannelHealth::Degraded {
            reason: "rate limited".to_owned()
        }
        .is_available());
        assert!(!ChannelHealth::Unavailable {
            reason: "disconnected".to_owned()
        }
        .is_available());
    }

    #[test]
    fn inbound_message_round_trip_with_thread() {
        let msg = InboundMessage::new("telegram", UserId::new("alice"), "hi").with_thread("t-1");
        let json = serde_json::to_string(&msg).unwrap();
        let back: InboundMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn inbound_message_round_trip_omits_absent_thread() {
        let msg = InboundMessage::new("cli", UserId::new("alice"), "hi");
        let json = serde_json::to_string(&msg).unwrap();
        // The optional thread is skipped when absent.
        assert!(!json.contains("thread"));
        let back: InboundMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn outbound_message_round_trip() {
        let msg = OutboundMessage::new(UserId::new("alice"), "result").with_thread("t-9");
        let json = serde_json::to_string(&msg).unwrap();
        let back: OutboundMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn channel_health_round_trip_each_variant() {
        for health in [
            ChannelHealth::Healthy,
            ChannelHealth::Degraded {
                reason: "slow".to_owned(),
            },
            ChannelHealth::Unavailable {
                reason: "down".to_owned(),
            },
        ] {
            let json = serde_json::to_string(&health).unwrap();
            let back: ChannelHealth = serde_json::from_str(&json).unwrap();
            assert_eq!(health, back);
        }
    }
}
