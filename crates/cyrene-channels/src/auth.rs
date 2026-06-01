//! Inbound authentication for channels: DM pairing + allowlist (R7.2, R22.5).
//!
//! Remote channels (Telegram, Slack, …) accept messages from anyone who can
//! reach the bot, so the gateway must authenticate the *sender* before a
//! message becomes a request. Cyrene uses two complementary mechanisms:
//!
//! - **Allowlist:** the config lists the sender ids permitted to talk to the
//!   agent. An empty allowlist denies everyone until the user opts senders in
//!   (secure-by-default).
//! - **DM pairing:** a one-time pairing code links a previously unknown sender
//!   to the authenticated user. Once paired, the sender id is trusted for
//!   future messages without re-entering the code.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// Decides whether an inbound sender is authorized to use the agent.
#[derive(Debug)]
pub struct InboundAuth {
    /// Sender ids explicitly allowed via config.
    allowlist: HashSet<String>,
    /// Senders paired at runtime via a pairing code.
    paired: Mutex<HashSet<String>>,
    /// Active one-time pairing codes mapped to the user they authorize.
    codes: Mutex<HashMap<String, String>>,
}

impl InboundAuth {
    /// Creates an auth gate from a static allowlist of sender ids.
    #[must_use]
    pub fn with_allowlist<I, S>(allow: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowlist: allow.into_iter().map(Into::into).collect(),
            paired: Mutex::new(HashSet::new()),
            codes: Mutex::new(HashMap::new()),
        }
    }

    /// Creates an empty, deny-all auth gate (secure-by-default).
    #[must_use]
    pub fn deny_all() -> Self {
        Self::with_allowlist(Vec::<String>::new())
    }

    /// Returns `true` if `sender` is currently authorized (allowlisted or
    /// paired).
    #[must_use]
    pub fn is_authorized(&self, sender: &str) -> bool {
        if self.allowlist.contains(sender) {
            return true;
        }
        self.paired.lock().unwrap().contains(sender)
    }

    /// Registers a one-time pairing `code` that, when redeemed, pairs a sender
    /// to `user`.
    pub fn issue_pairing_code(&self, code: impl Into<String>, user: impl Into<String>) {
        self.codes.lock().unwrap().insert(code.into(), user.into());
    }

    /// Redeems a pairing `code` for `sender`. On success the sender becomes
    /// authorized and the code is consumed. Returns the paired user id.
    ///
    /// # Errors
    /// Returns [`AuthError::UnknownCode`] if the code is invalid or already
    /// used.
    pub fn redeem(&self, sender: &str, code: &str) -> Result<String, AuthError> {
        let user = self
            .codes
            .lock()
            .unwrap()
            .remove(code)
            .ok_or(AuthError::UnknownCode)?;
        self.paired.lock().unwrap().insert(sender.to_owned());
        Ok(user)
    }
}

/// Errors from the inbound auth gate.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    /// The pairing code was invalid or already consumed.
    #[error("unknown or already-used pairing code")]
    UnknownCode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allowlist_denies_everyone() {
        let auth = InboundAuth::deny_all();
        assert!(!auth.is_authorized("anyone"));
    }

    #[test]
    fn allowlisted_sender_is_authorized() {
        let auth = InboundAuth::with_allowlist(["123", "456"]);
        assert!(auth.is_authorized("123"));
        assert!(!auth.is_authorized("789"));
    }

    #[test]
    fn pairing_code_authorizes_a_new_sender() {
        let auth = InboundAuth::deny_all();
        auth.issue_pairing_code("ABC123", "alice");
        assert!(!auth.is_authorized("tg:42"));

        let user = auth.redeem("tg:42", "ABC123").unwrap();
        assert_eq!(user, "alice");
        assert!(auth.is_authorized("tg:42"));
    }

    #[test]
    fn pairing_code_is_single_use() {
        let auth = InboundAuth::deny_all();
        auth.issue_pairing_code("ONE", "alice");
        assert!(auth.redeem("a", "ONE").is_ok());
        // Second redemption of the same code fails.
        assert_eq!(auth.redeem("b", "ONE").unwrap_err(), AuthError::UnknownCode);
    }

    #[test]
    fn wrong_code_is_rejected() {
        let auth = InboundAuth::deny_all();
        assert_eq!(
            auth.redeem("a", "nope").unwrap_err(),
            AuthError::UnknownCode
        );
    }
}
