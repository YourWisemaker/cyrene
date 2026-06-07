//! The [`Memory`] trait and its fact/query/result types.
//!
//! [`Memory`] is the backend-agnostic interface to the Memory_Graph (R16). The
//! default implementation (task 11) stores nodes and edges in SQLite, but the
//! trait is deliberately storage-neutral so a Maintainer can configure an
//! alternative backend without touching the core engine (R16.5). Facts are
//! upserted by a natural key so updates mutate the existing node rather than
//! creating a duplicate (R16.4), and relationship queries traverse edges
//! (R16.2).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{Recoverability, Recoverable};
use crate::ids::NodeId;

/// A node in the knowledge graph: an entity Cyrene has learned about.
///
/// `kind` groups entities (e.g. `"person"`, `"file"`, `"issue"`), `label` is a
/// human-readable name, and `props` carries arbitrary structured attributes.
/// The `(kind, label)` pair acts as the natural key used to dedup on upsert
/// (R16.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    /// The category of entity, e.g. `"person"`, `"file"`, `"issue"`.
    pub kind: String,
    /// A human-readable label/name for the entity.
    pub label: String,
    /// Arbitrary structured properties for the entity.
    pub props: serde_json::Value,
}

impl Fact {
    /// Creates a fact with the given kind, label, and properties.
    pub fn new(
        kind: impl Into<String>,
        label: impl Into<String>,
        props: serde_json::Value,
    ) -> Self {
        Self {
            kind: kind.into(),
            label: label.into(),
            props,
        }
    }
}

/// A typed relationship between two graph nodes (an edge label).
///
/// Kept as a stable string so new relationship kinds need no core change,
/// mirroring [`ChannelOrigin`](crate::ChannelOrigin).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Relation(pub String);

impl Relation {
    /// Creates a relation from anything string-like.
    pub fn new(rel: impl Into<String>) -> Self {
        Self(rel.into())
    }

    /// Returns the relation as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for Relation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl From<&str> for Relation {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for Relation {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// A query against the Memory_Graph.
///
/// A query can match by free text (FTS over labels/props), constrain by node
/// `kind`, and optionally traverse a relationship outward from an anchor node
/// to answer relationship questions (R16.2). All fields are optional so a
/// query can range from a broad text search to a precise edge traversal.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryQuery {
    /// Free-text to match against node labels/properties, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Restrict results to this node kind, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Anchor node to traverse relationships from, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<NodeId>,
    /// Relationship to traverse out of [`MemoryQuery::from`], if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<Relation>,
    /// Maximum number of hits to return, if capped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl MemoryQuery {
    /// Creates an empty query that matches everything (subject to a later
    /// limit). Use the builder-style setters to narrow it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Narrows the query to nodes matching the given free text.
    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Narrows the query to nodes of the given kind.
    #[must_use]
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Traverses `relation` outward from the `from` node.
    #[must_use]
    pub fn traversing(mut self, from: NodeId, relation: Relation) -> Self {
        self.from = Some(from);
        self.relation = Some(relation);
        self
    }

    /// Caps the number of hits returned.
    #[must_use]
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// A single result from a [`Memory::query`].
///
/// Carries the matched node (as a [`Fact`] plus its [`NodeId`]) and a relevance
/// `score` the backend assigns, so callers can rank results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryHit {
    /// The identifier of the matched node.
    pub id: NodeId,
    /// The matched node's content.
    pub fact: Fact,
    /// A backend-assigned relevance score (higher is more relevant).
    pub score: f32,
}

impl MemoryHit {
    /// Creates a hit for the given node, content, and relevance score.
    #[must_use]
    pub fn new(id: NodeId, fact: Fact, score: f32) -> Self {
        Self { id, fact, score }
    }
}

/// Errors a [`Memory`] implementation can return.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// A referenced node does not exist in the graph.
    #[error("memory node not found: {0}")]
    NodeNotFound(NodeId),

    /// The query was malformed or unsupported by the backend.
    #[error("invalid memory query: {0}")]
    InvalidQuery(String),

    /// An untrusted fact was refused because it carried prompt-injection
    /// patterns and must not be persisted into recallable memory (R21).
    ///
    /// The string summarizes which detection rules tripped, so the refusal can
    /// be logged without surfacing the malicious content itself.
    #[error("memory write quarantined: {0}")]
    Quarantined(String),

    /// A memory operation was refused because the requesting principal is not
    /// the owner of the memory. Defends against a hijacked or spoofed session
    /// manipulating memory it does not own.
    #[error("memory access unauthorized: {0}")]
    Unauthorized(String),

    /// The underlying storage backend failed.
    #[error("memory backend error: {0}")]
    Backend(String),

    /// A node's properties failed to (de)serialize.
    #[error("memory serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl Recoverable for MemoryError {
    fn recoverability(&self) -> Recoverability {
        match self {
            // A transient backend error (e.g. a locked DB) may succeed on retry.
            Self::Backend(_) => Recoverability::Retry,
            // A missing node or bad query needs the caller (or user) to correct.
            Self::NodeNotFound(_) | Self::InvalidQuery(_) => Recoverability::Halt,
            // A quarantined write is a deliberate security refusal: retrying the
            // same poisoned content would only fail again.
            Self::Quarantined(_) => Recoverability::Halt,
            // An unauthorized access is a security refusal, not a transient fault.
            Self::Unauthorized(_) => Recoverability::Halt,
            Self::Serialization(_) => Recoverability::Halt,
        }
    }
}

/// The Memory_Graph backend.
///
/// Registered in the Plugin_Registry and swappable via config without core
/// changes (R16.5).
#[async_trait]
pub trait Memory: Send + Sync {
    /// Stores a fact, updating the existing node when one matches the fact's
    /// natural key rather than creating a duplicate (R16.1, R16.4). Returns the
    /// id of the upserted node.
    ///
    /// # Errors
    /// Returns a [`MemoryError`] if the backend cannot store the fact.
    async fn upsert_fact(&self, fact: Fact) -> Result<NodeId, MemoryError>;

    /// Returns the nodes matching the query, traversing relationships when the
    /// query specifies them (R16.2).
    ///
    /// # Errors
    /// Returns a [`MemoryError`] if the query is invalid or the backend fails.
    async fn query(&self, q: MemoryQuery) -> Result<Vec<MemoryHit>, MemoryError>;

    /// Creates a `rel` relationship from node `from` to node `to` (R16.3).
    ///
    /// # Errors
    /// Returns a [`MemoryError`] if either endpoint is missing or the backend
    /// fails to record the edge.
    async fn link(&self, from: NodeId, rel: Relation, to: NodeId) -> Result<(), MemoryError>;
}

#[cfg(test)]
mod tests {
    use super::{Fact, MemoryHit, MemoryQuery, Relation};
    use crate::ids::NodeId;
    use serde_json::json;

    #[test]
    fn relation_transparent_string_round_trip() {
        let rel = Relation::new("authored");
        let json = serde_json::to_string(&rel).unwrap();
        // Transparent: a bare JSON string, no wrapping.
        assert_eq!(json, "\"authored\"");
        let back: Relation = serde_json::from_str("\"authored\"").unwrap();
        assert_eq!(back, rel);
        assert_eq!(back.as_str(), "authored");
    }

    #[test]
    fn relation_from_conversions_and_display() {
        assert_eq!(Relation::from("mentions").as_str(), "mentions");
        assert_eq!(Relation::from(String::from("owns")).as_str(), "owns");
        assert_eq!(Relation::new("links_to").to_string(), "links_to");
    }

    #[test]
    fn memory_query_builders_compose() {
        let anchor = NodeId::new();
        let q = MemoryQuery::new()
            .with_text("auth")
            .with_kind("file")
            .traversing(anchor, Relation::new("references"))
            .with_limit(10);
        assert_eq!(q.text.as_deref(), Some("auth"));
        assert_eq!(q.kind.as_deref(), Some("file"));
        assert_eq!(q.from, Some(anchor));
        assert_eq!(q.relation, Some(Relation::new("references")));
        assert_eq!(q.limit, Some(10));
    }

    #[test]
    fn fact_round_trip() {
        let fact = Fact::new("person", "Alice", json!({ "email": "alice@example.com" }));
        let json = serde_json::to_string(&fact).unwrap();
        let back: Fact = serde_json::from_str(&json).unwrap();
        assert_eq!(fact, back);
    }

    #[test]
    fn memory_query_round_trip_full() {
        let q = MemoryQuery::new()
            .with_text("budget guard")
            .with_kind("issue")
            .traversing(NodeId::new(), Relation::new("blocks"))
            .with_limit(5);
        let json = serde_json::to_string(&q).unwrap();
        let back: MemoryQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(q, back);
    }

    #[test]
    fn memory_query_round_trip_empty_omits_optionals() {
        let q = MemoryQuery::new();
        let json = serde_json::to_string(&q).unwrap();
        // An empty query serializes to an empty object; all fields are skipped.
        assert_eq!(json, "{}");
        let back: MemoryQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(q, back);
    }

    #[test]
    fn memory_hit_round_trip() {
        let hit = MemoryHit::new(
            NodeId::new(),
            Fact::new("file", "main.rs", json!({ "lines": 42 })),
            0.87,
        );
        let json = serde_json::to_string(&hit).unwrap();
        let back: MemoryHit = serde_json::from_str(&json).unwrap();
        assert_eq!(hit, back);
    }
}
