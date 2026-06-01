//! Shared session store for cross-channel continuity (R7.5).
//!
//! When a user continues a conversation on a different channel, the gateway
//! must keep the same [`SessionId`] so context carries over. The store maps a
//! stable **conversation key** — derived from the user (and an optional thread)
//! rather than from the channel — to a session id. Two messages from the same
//! user resolve to the same session even when they arrive on different
//! channels.

use std::collections::HashMap;
use std::sync::Mutex;

use cyrene_core::{InboundMessage, SessionId, UserId};

/// A conversation key that is stable across channels for the same user.
///
/// Keyed by user id (and thread when present) but **not** by channel, so a
/// follow-up on another channel maps to the same session (R7.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConversationKey {
    user_id: UserId,
    thread: Option<String>,
}

impl ConversationKey {
    /// Derives the conversation key for an inbound message.
    #[must_use]
    pub fn of(msg: &InboundMessage) -> Self {
        Self {
            user_id: msg.user_id.clone(),
            thread: msg.thread.clone(),
        }
    }
}

/// Maps conversation keys to session ids, shared across all channels.
#[derive(Debug, Default)]
pub struct SessionStore {
    map: Mutex<HashMap<ConversationKey, SessionId>>,
}

impl SessionStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the session id for an inbound message, creating and remembering
    /// a fresh one if this is the conversation's first message.
    ///
    /// Because the key ignores the channel, a continuation on a different
    /// channel resolves to the same session (R7.5).
    #[must_use]
    pub fn session_for(&self, msg: &InboundMessage) -> SessionId {
        let key = ConversationKey::of(msg);
        let mut map = self.map.lock().expect("session store mutex poisoned");
        *map.entry(key).or_default()
    }

    /// Returns the existing session id for a message, if one was already
    /// assigned, without creating a new one.
    #[must_use]
    pub fn existing(&self, msg: &InboundMessage) -> Option<SessionId> {
        let key = ConversationKey::of(msg);
        let map = self.map.lock().expect("session store mutex poisoned");
        map.get(&key).copied()
    }

    /// The number of tracked conversations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.lock().expect("session store mutex poisoned").len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_user_same_thread_reuses_session_across_channels() {
        let store = SessionStore::new();
        // Same user + thread, different channels (telegram then slack).
        let on_tg =
            InboundMessage::new("telegram", UserId::new("alice"), "start").with_thread("t-1");
        let on_slack =
            InboundMessage::new("slack", UserId::new("alice"), "continue").with_thread("t-1");

        let s1 = store.session_for(&on_tg);
        let s2 = store.session_for(&on_slack);
        assert_eq!(s1, s2, "continuation on another channel keeps the session");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn different_users_get_distinct_sessions() {
        let store = SessionStore::new();
        let a = InboundMessage::new("cli", UserId::new("alice"), "hi");
        let b = InboundMessage::new("cli", UserId::new("bob"), "hi");
        assert_ne!(store.session_for(&a), store.session_for(&b));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn distinct_threads_are_distinct_conversations() {
        let store = SessionStore::new();
        let t1 = InboundMessage::new("slack", UserId::new("alice"), "a").with_thread("1");
        let t2 = InboundMessage::new("slack", UserId::new("alice"), "b").with_thread("2");
        assert_ne!(store.session_for(&t1), store.session_for(&t2));
    }

    #[test]
    fn existing_returns_none_then_some() {
        let store = SessionStore::new();
        let msg = InboundMessage::new("cli", UserId::new("alice"), "hi");
        assert!(store.existing(&msg).is_none());
        let s = store.session_for(&msg);
        assert_eq!(store.existing(&msg), Some(s));
    }
}
