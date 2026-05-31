//! Stable identifier newtypes.
//!
//! Domain entities are keyed by typed wrappers over [`Uuid`] rather than bare
//! UUIDs so the compiler prevents mixing, for example, a [`SessionId`] with a
//! [`PlanId`]. Each type round-trips through serde as a plain UUID string.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Declares a UUID-backed identifier newtype with common conveniences.
macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Generates a fresh, random (v4) identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wraps an existing [`Uuid`].
            #[must_use]
            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            /// Returns the underlying [`Uuid`].
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }

        impl From<Uuid> for $name {
            fn from(id: Uuid) -> Self {
                Self(id)
            }
        }
    };
}

uuid_id!(
    /// Identifies a [`Session`](crate::Session).
    SessionId
);
uuid_id!(
    /// Identifies a [`Plan`](crate::Plan).
    PlanId
);
uuid_id!(
    /// Identifies a branch within a session's State_Tree.
    BranchId
);
uuid_id!(
    /// Identifies a node in the [`Memory`](crate::Memory) graph.
    NodeId
);

/// Identifies the originating channel of a session or message (e.g. `"cli"`,
/// `"telegram"`). Kept as a stable string so new channels need no core change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelOrigin(pub String);

impl ChannelOrigin {
    /// Creates a channel origin from anything string-like.
    pub fn new(origin: impl Into<String>) -> Self {
        Self(origin.into())
    }

    /// Returns the origin as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for ChannelOrigin {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl From<&str> for ChannelOrigin {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for ChannelOrigin {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Identifies the user a session belongs to. A stable string so it can hold a
/// channel-native handle or an internal account id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub String);

impl UserId {
    /// Creates a user id from anything string-like.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for UserId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl From<&str> for UserId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for UserId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[cfg(test)]
mod tests {
    use super::{BranchId, ChannelOrigin, NodeId, PlanId, SessionId, UserId};
    use uuid::Uuid;

    #[test]
    fn uuid_ids_round_trip() {
        let id = SessionId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: SessionId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn uuid_ids_serialize_transparently_as_plain_uuid_string() {
        let raw = Uuid::new_v4();
        let id = PlanId::from_uuid(raw);
        let json = serde_json::to_string(&id).unwrap();
        // Transparent: the JSON is exactly the quoted UUID string, with no
        // wrapping object or tuple.
        assert_eq!(json, format!("\"{raw}\""));
        // And it deserializes from a bare UUID string.
        let back: PlanId = serde_json::from_str(&format!("\"{raw}\"")).unwrap();
        assert_eq!(back.as_uuid(), raw);
    }

    #[test]
    fn all_uuid_ids_serialize_transparently_as_plain_uuid_string() {
        // Every UUID-backed id newtype shares the transparent representation.
        let raw = Uuid::new_v4();
        let quoted = format!("\"{raw}\"");

        assert_eq!(
            serde_json::to_string(&SessionId::from_uuid(raw)).unwrap(),
            quoted
        );
        assert_eq!(
            serde_json::to_string(&BranchId::from_uuid(raw)).unwrap(),
            quoted
        );
        assert_eq!(
            serde_json::to_string(&NodeId::from_uuid(raw)).unwrap(),
            quoted
        );

        // ...and each deserializes back from the bare UUID string.
        assert_eq!(
            serde_json::from_str::<SessionId>(&quoted)
                .unwrap()
                .as_uuid(),
            raw
        );
        assert_eq!(
            serde_json::from_str::<BranchId>(&quoted).unwrap().as_uuid(),
            raw
        );
        assert_eq!(
            serde_json::from_str::<NodeId>(&quoted).unwrap().as_uuid(),
            raw
        );
    }

    #[test]
    fn branch_and_node_ids_round_trip() {
        let branch = BranchId::new();
        let node = NodeId::new();
        let branch_back: BranchId =
            serde_json::from_str(&serde_json::to_string(&branch).unwrap()).unwrap();
        let node_back: NodeId =
            serde_json::from_str(&serde_json::to_string(&node).unwrap()).unwrap();
        assert_eq!(branch, branch_back);
        assert_eq!(node, node_back);
    }

    #[test]
    fn from_uuid_and_as_uuid_round_trip() {
        let raw = Uuid::new_v4();
        assert_eq!(NodeId::from_uuid(raw).as_uuid(), raw);
        assert_eq!(BranchId::from(raw).as_uuid(), raw);
    }

    #[test]
    fn new_ids_are_unique() {
        assert_ne!(SessionId::new(), SessionId::new());
    }

    #[test]
    fn display_matches_inner_uuid() {
        let raw = Uuid::new_v4();
        let id = SessionId::from_uuid(raw);
        assert_eq!(id.to_string(), raw.to_string());
    }

    #[test]
    fn channel_origin_serializes_transparently_as_plain_string() {
        let origin = ChannelOrigin::new("telegram");
        let json = serde_json::to_string(&origin).unwrap();
        assert_eq!(json, "\"telegram\"");
        let back: ChannelOrigin = serde_json::from_str("\"telegram\"").unwrap();
        assert_eq!(back, origin);
        assert_eq!(back.as_str(), "telegram");
    }

    #[test]
    fn channel_origin_from_conversions() {
        assert_eq!(ChannelOrigin::from("cli").as_str(), "cli");
        assert_eq!(ChannelOrigin::from(String::from("slack")).as_str(), "slack");
        assert_eq!(ChannelOrigin::new("cli").to_string(), "cli");
    }

    #[test]
    fn user_id_serializes_transparently_as_plain_string() {
        let user = UserId::new("alice");
        let json = serde_json::to_string(&user).unwrap();
        assert_eq!(json, "\"alice\"");
        let back: UserId = serde_json::from_str("\"alice\"").unwrap();
        assert_eq!(back, user);
        assert_eq!(back.as_str(), "alice");
    }

    #[test]
    fn user_id_from_conversions() {
        assert_eq!(UserId::from("bob").as_str(), "bob");
        assert_eq!(UserId::from(String::from("carol")).as_str(), "carol");
        assert_eq!(UserId::new("dave").to_string(), "dave");
    }
}
