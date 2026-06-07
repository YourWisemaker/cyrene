//! Optional Redis-backed [`Memory`] backend (R16.5).
//!
//! This module is compiled **only** when the crate's `redis` feature is enabled
//! ("only if redis exists"), and the backend is only *instantiated* when a user
//! declares a `[memory.redis.<alias>]` entry in their config ("and the user
//! commands it"). SQLite remains the built-in default; nothing here is reached
//! unless both conditions hold.
//!
//! # Data model
//!
//! The graph is mapped onto plain Redis keys so it works against any vanilla
//! Redis/Valkey server (no modules like RediSearch required). Every key is
//! namespaced by a configurable prefix (default `cyrene`):
//!
//! - `‹p›:node:‹id›` — a HASH with fields `kind`, `label`, `props` (JSON).
//! - `‹p›:nk:‹kind›:‹label›` — a STRING holding the node id; the natural-key
//!   index that powers dedup-on-upsert (R16.4).
//! - `‹p›:kind:‹kind›` — a SET of node ids of that kind (kind listing/filter).
//! - `‹p›:nodes` — a SET of every node id (full listing + free-text scan).
//! - `‹p›:edge:‹from›:‹rel›` — a SET of `to` node ids for one relation (R16.3).
//!
//! # Connection handling
//!
//! A [`redis::aio::ConnectionManager`] is created lazily on first use and cached
//! in a [`OnceCell`]. The manager is cheap to clone and transparently
//! reconnects, so each trait method clones it and issues its commands without
//! holding a lock across `await`.
//!
//! # Search note
//!
//! Free-text recall scans node labels/props with a case-insensitive substring
//! match rather than a real index. That is adequate for an opt-in backend; a
//! deployment that needs ranked search should front Redis with RediSearch or
//! keep the SQLite/FTS5 default.

use std::collections::HashMap;

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use tokio::sync::OnceCell;

use cyrene_core::{Fact, Memory, MemoryError, MemoryHit, MemoryQuery, NodeId, Relation};

/// The default key namespace, used when none is supplied.
const DEFAULT_PREFIX: &str = "cyrene";

/// A Redis/Valkey-backed knowledge graph implementing the [`Memory`] trait.
///
/// Construct it with [`RedisMemory::connect`] (eagerly establishing a
/// connection) or [`RedisMemory::new`] (deferring the connection until the first
/// operation, which is what a sync component factory wants).
#[derive(Clone)]
pub struct RedisMemory {
    client: redis::Client,
    conn: OnceCell<ConnectionManager>,
    prefix: String,
}

impl std::fmt::Debug for RedisMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisMemory")
            .field("prefix", &self.prefix)
            .field("connected", &self.conn.initialized())
            .finish_non_exhaustive()
    }
}

impl RedisMemory {
    /// Builds a backend for `url` (e.g. `redis://127.0.0.1/`) **without** opening
    /// a connection yet. The connection is established lazily on the first
    /// operation, so this is safe to call from a synchronous component factory.
    ///
    /// # Errors
    /// Returns [`MemoryError::Backend`] if `url` is not a valid Redis URL.
    pub fn new(url: &str) -> Result<Self, MemoryError> {
        let client = redis::Client::open(url).map_err(redis_err)?;
        Ok(Self {
            client,
            conn: OnceCell::new(),
            prefix: DEFAULT_PREFIX.to_owned(),
        })
    }

    /// Builds a backend for `url` and eagerly opens the connection so a failure
    /// surfaces immediately. Intended for callers already in an async context.
    ///
    /// # Errors
    /// Returns [`MemoryError::Backend`] if the URL is invalid or the server is
    /// unreachable.
    pub async fn connect(url: &str) -> Result<Self, MemoryError> {
        let this = Self::new(url)?;
        this.conn().await?; // force the lazy connection now
        Ok(this)
    }

    /// Overrides the key namespace prefix (default `cyrene`). Use this to run
    /// several isolated graphs against one Redis instance.
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Returns a cloned, ready connection, establishing it on first use.
    async fn conn(&self) -> Result<ConnectionManager, MemoryError> {
        let cm = self
            .conn
            .get_or_try_init(|| async { self.client.get_connection_manager().await })
            .await
            .map_err(redis_err)?;
        Ok(cm.clone())
    }

    // --- key builders ---

    fn key_node(&self, id: &str) -> String {
        format!("{}:node:{id}", self.prefix)
    }
    fn key_nk(&self, kind: &str, label: &str) -> String {
        format!("{}:nk:{kind}:{label}", self.prefix)
    }
    fn key_kind(&self, kind: &str) -> String {
        format!("{}:kind:{kind}", self.prefix)
    }
    fn key_nodes(&self) -> String {
        format!("{}:nodes", self.prefix)
    }
    fn key_edge(&self, from: &str, rel: &str) -> String {
        format!("{}:edge:{from}:{rel}", self.prefix)
    }

    /// Loads a node by id string into a `(NodeId, Fact)`, or `None` if absent.
    async fn load_node(
        &self,
        conn: &mut ConnectionManager,
        id_str: &str,
    ) -> Result<Option<(NodeId, Fact)>, MemoryError> {
        let map: HashMap<String, String> = conn
            .hgetall(self.key_node(id_str))
            .await
            .map_err(redis_err)?;
        if map.is_empty() {
            return Ok(None);
        }
        let kind = map
            .get("kind")
            .ok_or_else(|| MemoryError::Backend(format!("node {id_str} missing kind")))?;
        let label = map
            .get("label")
            .ok_or_else(|| MemoryError::Backend(format!("node {id_str} missing label")))?;
        let props_str = map.get("props").map_or("null", String::as_str);
        let props: serde_json::Value = serde_json::from_str(props_str)?;
        Ok(Some((
            parse_node_id(id_str)?,
            Fact::new(kind.clone(), label.clone(), props),
        )))
    }
}

#[async_trait]
impl Memory for RedisMemory {
    async fn upsert_fact(&self, fact: Fact) -> Result<NodeId, MemoryError> {
        let mut conn = self.conn().await?;
        let props_str = serde_json::to_string(&fact.props)?;
        let nk = self.key_nk(&fact.kind, &fact.label);

        // Natural-key lookup → dedup-on-upsert (R16.4).
        let existing: Option<String> = conn.get(&nk).await.map_err(redis_err)?;

        match existing {
            Some(id_str) => {
                // Mutate the existing node's props in place; key fields are
                // immutable since they are the identity.
                let _: () = conn
                    .hset(self.key_node(&id_str), "props", &props_str)
                    .await
                    .map_err(redis_err)?;
                parse_node_id(&id_str)
            }
            None => {
                let id = NodeId::new();
                let id_str = id.to_string();
                let node_key = self.key_node(&id_str);
                let fields = [
                    ("kind", fact.kind.as_str()),
                    ("label", fact.label.as_str()),
                    ("props", props_str.as_str()),
                ];
                let _: () = conn
                    .hset_multiple(&node_key, &fields)
                    .await
                    .map_err(redis_err)?;
                let _: () = conn.set(&nk, &id_str).await.map_err(redis_err)?;
                let _: () = conn
                    .sadd(self.key_kind(&fact.kind), &id_str)
                    .await
                    .map_err(redis_err)?;
                let _: () = conn
                    .sadd(self.key_nodes(), &id_str)
                    .await
                    .map_err(redis_err)?;
                Ok(id)
            }
        }
    }

    async fn query(&self, q: MemoryQuery) -> Result<Vec<MemoryHit>, MemoryError> {
        let mut conn = self.conn().await?;
        let limit = q.limit.unwrap_or(usize::MAX);
        let mut hits: Vec<MemoryHit> = Vec::new();

        match (&q.from, &q.relation, &q.text) {
            // Relationship traversal outward from `from` along `relation` (R16.2).
            (Some(from), Some(rel), _) => {
                let to_ids: Vec<String> = conn
                    .smembers(self.key_edge(&from.to_string(), rel.as_str()))
                    .await
                    .map_err(redis_err)?;
                for to_id in to_ids {
                    if let Some((id, fact)) = self.load_node(&mut conn, &to_id).await? {
                        if kind_matches(&fact, q.kind.as_deref()) {
                            hits.push(MemoryHit::new(id, fact, 1.0));
                        }
                    }
                }
            }
            // A traversal needs both anchor and relation, mirroring the SQLite backend.
            (Some(_), None, _) | (None, Some(_), _) => {
                return Err(MemoryError::InvalidQuery(
                    "traversal requires both `from` and `relation`".to_owned(),
                ));
            }
            // Free-text recall: case-insensitive substring scan over label/props (R16.2).
            (None, None, Some(text)) if !text.trim().is_empty() => {
                let needle = text.to_lowercase();
                let all: Vec<String> = conn.smembers(self.key_nodes()).await.map_err(redis_err)?;
                for id_str in all {
                    if let Some((id, fact)) = self.load_node(&mut conn, &id_str).await? {
                        let haystack = format!("{} {}", fact.label, fact.props).to_lowercase();
                        if haystack.contains(&needle) && kind_matches(&fact, q.kind.as_deref()) {
                            hits.push(MemoryHit::new(id, fact, 1.0));
                        }
                    }
                }
            }
            // No text and no traversal: list nodes, optionally filtered by kind.
            _ => {
                let ids: Vec<String> = match q.kind.as_deref() {
                    Some(kind) => conn
                        .smembers(self.key_kind(kind))
                        .await
                        .map_err(redis_err)?,
                    None => conn.smembers(self.key_nodes()).await.map_err(redis_err)?,
                };
                for id_str in ids {
                    if let Some((id, fact)) = self.load_node(&mut conn, &id_str).await? {
                        hits.push(MemoryHit::new(id, fact, 1.0));
                    }
                }
                // Stable ordering by label, matching the SQLite backend's ORDER BY.
                hits.sort_by(|a, b| a.fact.label.cmp(&b.fact.label));
            }
        }

        hits.truncate(limit);
        Ok(hits)
    }

    async fn link(&self, from: NodeId, rel: Relation, to: NodeId) -> Result<(), MemoryError> {
        let mut conn = self.conn().await?;

        // Both endpoints must exist before an edge is recorded (R16.3).
        let from_exists: bool = conn
            .exists(self.key_node(&from.to_string()))
            .await
            .map_err(redis_err)?;
        if !from_exists {
            return Err(MemoryError::NodeNotFound(from));
        }
        let to_exists: bool = conn
            .exists(self.key_node(&to.to_string()))
            .await
            .map_err(redis_err)?;
        if !to_exists {
            return Err(MemoryError::NodeNotFound(to));
        }

        // SADD is idempotent, so duplicate (from, rel, to) edges collapse.
        let _: () = conn
            .sadd(
                self.key_edge(&from.to_string(), rel.as_str()),
                to.to_string(),
            )
            .await
            .map_err(redis_err)?;
        Ok(())
    }
}

/// Maps a `redis` error into a retryable [`MemoryError::Backend`].
fn redis_err(e: redis::RedisError) -> MemoryError {
    MemoryError::Backend(e.to_string())
}

/// Parses a stored UUID string into a [`NodeId`].
fn parse_node_id(s: &str) -> Result<NodeId, MemoryError> {
    s.parse::<uuid::Uuid>()
        .map(NodeId::from_uuid)
        .map_err(|_| MemoryError::Backend(format!("corrupt node id: {s}")))
}

/// Returns `true` if `fact.kind` matches the optional kind filter.
fn kind_matches(fact: &Fact, kind: Option<&str>) -> bool {
    kind.is_none_or(|k| fact.kind == k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_url_is_a_backend_error() {
        let err = RedisMemory::new("not-a-redis-url").unwrap_err();
        assert!(matches!(err, MemoryError::Backend(_)));
    }

    #[test]
    fn prefix_defaults_and_overrides() {
        let m = RedisMemory::new("redis://127.0.0.1/").unwrap();
        assert_eq!(m.key_nodes(), "cyrene:nodes");
        let m = m.with_prefix("test");
        assert_eq!(m.key_nodes(), "test:nodes");
        assert_eq!(m.key_node("abc"), "test:node:abc");
        assert_eq!(m.key_edge("a", "knows"), "test:edge:a:knows");
    }

    // Integration tests against a live server are opt-in: they require a Redis
    // instance on REDIS_URL (or localhost) and are ignored by default so CI
    // without a server stays green. Run with:
    //   REDIS_URL=redis://127.0.0.1/ cargo test -p cyrene-memory --features redis -- --ignored
    #[tokio::test]
    #[ignore = "requires a running Redis server"]
    async fn upsert_query_link_roundtrip() {
        use cyrene_core::MemoryQuery;
        use serde_json::json;

        let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_owned());
        // Unique prefix so repeated runs don't collide.
        let prefix = format!("cyrene-test-{}", uuid::Uuid::new_v4());
        let mem = RedisMemory::connect(&url)
            .await
            .unwrap()
            .with_prefix(prefix);

        let alice = mem
            .upsert_fact(Fact::new("person", "Alice", json!({"role": "dev"})))
            .await
            .unwrap();
        // Dedup-on-upsert: same (kind,label) returns the same id.
        let alice2 = mem
            .upsert_fact(Fact::new("person", "Alice", json!({"role": "lead"})))
            .await
            .unwrap();
        assert_eq!(alice, alice2);

        let issue = mem
            .upsert_fact(Fact::new("issue", "ISSUE-1", json!({"title": "bug"})))
            .await
            .unwrap();
        mem.link(alice, Relation::new("assigned_to"), issue)
            .await
            .unwrap();

        let traversed = mem
            .query(MemoryQuery::new().traversing(alice, Relation::new("assigned_to")))
            .await
            .unwrap();
        assert_eq!(traversed.len(), 1);
        assert_eq!(traversed[0].id, issue);

        let by_text = mem
            .query(MemoryQuery::new().with_text("Alice"))
            .await
            .unwrap();
        assert!(by_text.iter().any(|h| h.id == alice));
        assert_eq!(
            by_text.iter().find(|h| h.id == alice).unwrap().fact.props["role"],
            json!("lead")
        );
    }
}
