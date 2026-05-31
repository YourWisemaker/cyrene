//! Sessions.
//!
//! A [`Session`] is a bounded unit of work with its own [`Budget`], State_Tree
//! branch, and conversation context (per the glossary). It is the anchor the
//! ledger, state tree, and budget guard all key off of.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::budget::Budget;
use crate::ids::{BranchId, ChannelOrigin, SessionId, UserId};

/// A bounded unit of work owned by a user and started from a channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Unique identifier for this session.
    pub id: SessionId,
    /// The user the session belongs to.
    pub user_id: UserId,
    /// The channel the session originated on (responses default here, R7.4).
    pub channel_origin: ChannelOrigin,
    /// The session's budget guardrails and accumulated usage.
    pub budget: Budget,
    /// The current State_Tree branch this session is advancing on.
    pub branch_id: BranchId,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
}

impl Session {
    /// Creates a session with fresh ids and the given budget, started now.
    #[must_use]
    pub fn new(user_id: UserId, channel_origin: ChannelOrigin, budget: Budget) -> Self {
        Self {
            id: SessionId::new(),
            user_id,
            channel_origin,
            budget,
            branch_id: BranchId::new(),
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    use crate::budget::Budget;
    use crate::ids::{ChannelOrigin, UserId};
    use crate::money::Money;
    use std::time::Duration;

    #[test]
    fn new_populates_fields_and_fresh_ids() {
        let session = Session::new(
            UserId::new("alice"),
            ChannelOrigin::new("cli"),
            Budget::unlimited(),
        );
        assert_eq!(session.user_id, UserId::new("alice"));
        assert_eq!(session.channel_origin, ChannelOrigin::new("cli"));
        // Two fresh sessions get distinct ids and branches.
        let other = Session::new(
            UserId::new("alice"),
            ChannelOrigin::new("cli"),
            Budget::unlimited(),
        );
        assert_ne!(session.id, other.id);
        assert_ne!(session.branch_id, other.branch_id);
    }

    #[test]
    fn serde_round_trip() {
        let budget = Budget::new(
            Some(Money::new("USD", 5000)),
            Some(50_000),
            Some(Duration::from_secs(600)),
        );
        let session = Session::new(UserId::new("alice"), ChannelOrigin::new("telegram"), budget);
        let json = serde_json::to_string(&session).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(session, back);
    }
}
