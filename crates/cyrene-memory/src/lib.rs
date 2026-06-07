//! `cyrene-memory`: the SQLite-backed Memory_Graph for Cyrene (R16).
//!
//! [`MemoryGraph`] implements the backend-agnostic [`Memory`] trait defined in
//! `cyrene-core`. It stores entities as graph **nodes** and typed
//! relationships as **edges** in a single embedded SQLite database (via
//! `rusqlite` with the `bundled` feature, so no system SQLite is required).
//!
//! Design highlights:
//!
//! - **Dedup on upsert (R16.4):** a node's `(kind, label)` pair is its natural
//!   key. [`MemoryGraph::upsert_fact`] updates the existing node's properties
//!   in place when the key already exists rather than creating a duplicate, and
//!   returns the same [`NodeId`].
//! - **Relationship traversal (R16.2/16.3):** edges connect nodes with a string
//!   relation label, so a query can traverse outward from an anchor node to
//!   answer relationship questions ("which work does this file appear in?").
//! - **Free-text recall (R16.2):** an FTS5 virtual table indexes each node's
//!   label and properties, so a text query matches relevant nodes.
//! - **Swappable (R16.5):** because it implements the `Memory` trait, an
//!   alternative backend can be registered via config without touching the
//!   core engine.

use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension};

use cyrene_core::{Fact, Memory, MemoryError, MemoryHit, MemoryQuery, NodeId, Relation};

pub mod authz;
pub mod guard;

#[cfg(feature = "redis")]
pub mod redis;

pub use authz::{owner_of, AuthorizedMemory};
pub use guard::{is_untrusted, provenance, GuardedMemory};

#[cfg(feature = "redis")]
pub use redis::RedisMemory;

/// The schema for the memory graph.
///
/// `nodes` holds entities keyed by a generated id with a `UNIQUE(kind, label)`
/// natural key for dedup-on-upsert. `edges` holds typed relationships with a
/// `UNIQUE(from_id, rel, to_id)` constraint so links are idempotent.
/// `nodes_fts` is an FTS5 index over each node's label and properties for
/// free-text recall, kept in sync manually on each upsert.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS nodes (
    id    TEXT PRIMARY KEY,
    kind  TEXT NOT NULL,
    label TEXT NOT NULL,
    props TEXT NOT NULL,
    UNIQUE (kind, label)
);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes (kind);

CREATE TABLE IF NOT EXISTS edges (
    from_id TEXT NOT NULL,
    rel     TEXT NOT NULL,
    to_id   TEXT NOT NULL,
    UNIQUE (from_id, rel, to_id)
);
CREATE INDEX IF NOT EXISTS idx_edges_from ON edges (from_id, rel);
CREATE INDEX IF NOT EXISTS idx_edges_to   ON edges (to_id, rel);

CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts
    USING fts5(node_id UNINDEXED, label, props);";

/// A SQLite-backed knowledge graph implementing the [`Memory`] trait.
///
/// The `rusqlite` connection is sync, so it is guarded by a [`Mutex`]; each
/// trait method locks the connection, performs its work synchronously, and
/// releases the lock before returning. No `await` points are held across the
/// lock.
#[derive(Debug)]
pub struct MemoryGraph {
    conn: Mutex<Connection>,
}

impl MemoryGraph {
    /// Opens (or creates) a memory graph at `db_path`, initializing the schema
    /// if absent.
    ///
    /// # Errors
    /// Returns [`MemoryError::Backend`] if the database cannot be opened or the
    /// schema cannot be initialized.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, MemoryError> {
        let conn = Connection::open(db_path).map_err(db_err)?;
        Self::from_connection(conn)
    }

    /// Opens an in-memory memory graph. Intended for tests.
    ///
    /// # Errors
    /// Returns [`MemoryError::Backend`] if the schema cannot be initialized.
    pub fn in_memory() -> Result<Self, MemoryError> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        Self::from_connection(conn)
    }

    /// Initializes the schema and wraps the connection.
    fn from_connection(conn: Connection) -> Result<Self, MemoryError> {
        conn.execute_batch(SCHEMA).map_err(db_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Locks the connection, mapping a poisoned mutex to a backend error.
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, MemoryError> {
        self.conn
            .lock()
            .map_err(|_| MemoryError::Backend("memory graph mutex poisoned".to_owned()))
    }

    /// Upserts `entity`, then links it to the `work` node via `rel` (R16.3).
    ///
    /// Convenience for the common pattern of recording a file/person/issue and
    /// connecting it to the work it appears in. Returns the (possibly existing)
    /// entity node id.
    ///
    /// # Errors
    /// Returns [`MemoryError`] if the upsert fails, or [`MemoryError::NodeNotFound`]
    /// if `work` does not exist.
    pub async fn link_entity_to_work(
        &self,
        entity: Fact,
        rel: Relation,
        work: NodeId,
    ) -> Result<NodeId, MemoryError> {
        let entity_id = self.upsert_fact(entity).await?;
        self.link(entity_id, rel, work).await?;
        Ok(entity_id)
    }
}

#[async_trait]
impl Memory for MemoryGraph {
    async fn upsert_fact(&self, fact: Fact) -> Result<NodeId, MemoryError> {
        let conn = self.lock()?;
        let props_str = serde_json::to_string(&fact.props)?;

        // Look up an existing node by its natural key (kind, label).
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM nodes WHERE kind = ?1 AND label = ?2",
                rusqlite::params![fact.kind, fact.label],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?;

        match existing {
            // Dedup-on-update: mutate the existing node's properties (R16.4).
            Some(id_str) => {
                conn.execute(
                    "UPDATE nodes SET props = ?1 WHERE id = ?2",
                    rusqlite::params![props_str, id_str],
                )
                .map_err(db_err)?;
                sync_fts(&conn, &id_str, &fact.label, &props_str)?;
                parse_node_id(&id_str)
            }
            // New entity: generate a fresh id and insert.
            None => {
                let id = NodeId::new();
                let id_str = id.to_string();
                conn.execute(
                    "INSERT INTO nodes (id, kind, label, props) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![id_str, fact.kind, fact.label, props_str],
                )
                .map_err(db_err)?;
                conn.execute(
                    "INSERT INTO nodes_fts (node_id, label, props) VALUES (?1, ?2, ?3)",
                    rusqlite::params![id_str, fact.label, props_str],
                )
                .map_err(db_err)?;
                Ok(id)
            }
        }
    }

    async fn query(&self, q: MemoryQuery) -> Result<Vec<MemoryHit>, MemoryError> {
        let conn = self.lock()?;
        let limit = q.limit.unwrap_or(usize::MAX);
        let mut hits: Vec<MemoryHit> = Vec::new();

        match (&q.from, &q.relation, &q.text) {
            // Relationship traversal: follow `relation` outward from `from` (R16.2).
            (Some(from), Some(rel), _) => {
                let mut stmt = conn
                    .prepare("SELECT to_id FROM edges WHERE from_id = ?1 AND rel = ?2")
                    .map_err(db_err)?;
                let to_ids = stmt
                    .query_map(rusqlite::params![from.to_string(), rel.as_str()], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(db_err)?
                    .collect::<Result<Vec<String>, _>>()
                    .map_err(db_err)?;

                for to_id in to_ids {
                    if let Some((id, fact)) = load_node(&conn, &to_id)? {
                        if kind_matches(&fact, q.kind.as_deref()) {
                            hits.push(MemoryHit::new(id, fact, 1.0));
                        }
                    }
                }
            }
            // A traversal anchor without a relation (or vice versa) is invalid.
            (Some(_), None, _) | (None, Some(_), _) => {
                return Err(MemoryError::InvalidQuery(
                    "traversal requires both `from` and `relation`".to_owned(),
                ));
            }
            // Free-text recall over labels and properties (R16.2).
            (None, None, Some(text)) if !text.trim().is_empty() => {
                let match_expr = fts_phrase(text);
                let mut stmt = conn
                    .prepare(
                        "SELECT node_id, rank FROM nodes_fts \
                         WHERE nodes_fts MATCH ?1 ORDER BY rank",
                    )
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map([match_expr], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                    })
                    .map_err(db_err)?
                    .collect::<Result<Vec<(String, f64)>, _>>()
                    .map_err(db_err)?;

                for (node_id, rank) in rows {
                    if let Some((id, fact)) = load_node(&conn, &node_id)? {
                        if kind_matches(&fact, q.kind.as_deref()) {
                            // FTS5 `rank` (bm25) is negative; flip so higher = better.
                            hits.push(MemoryHit::new(id, fact, -rank as f32));
                        }
                    }
                }
            }
            // No text and no traversal: list nodes, optionally filtered by kind.
            _ => match q.kind.as_deref() {
                Some(kind) => {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id, kind, label, props FROM nodes \
                                 WHERE kind = ?1 ORDER BY label",
                        )
                        .map_err(db_err)?;
                    let rows = stmt.query_map([kind], row_to_node).map_err(db_err)?;
                    hits = rows_to_hits(rows)?;
                }
                None => {
                    let mut stmt = conn
                        .prepare("SELECT id, kind, label, props FROM nodes ORDER BY label")
                        .map_err(db_err)?;
                    let rows = stmt.query_map([], row_to_node).map_err(db_err)?;
                    hits = rows_to_hits(rows)?;
                }
            },
        }

        hits.truncate(limit);
        Ok(hits)
    }

    async fn link(&self, from: NodeId, rel: Relation, to: NodeId) -> Result<(), MemoryError> {
        let conn = self.lock()?;

        // Both endpoints must exist before an edge is recorded (R16.3).
        if !node_exists(&conn, &from.to_string())? {
            return Err(MemoryError::NodeNotFound(from));
        }
        if !node_exists(&conn, &to.to_string())? {
            return Err(MemoryError::NodeNotFound(to));
        }

        // Idempotent insert: duplicate (from, rel, to) edges are ignored.
        conn.execute(
            "INSERT OR IGNORE INTO edges (from_id, rel, to_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![from.to_string(), rel.as_str(), to.to_string()],
        )
        .map_err(db_err)?;

        Ok(())
    }
}

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-memory"
}

// --- internal helpers ---

/// Maps a `rusqlite` error into a [`MemoryError::Backend`].
fn db_err(e: rusqlite::Error) -> MemoryError {
    MemoryError::Backend(e.to_string())
}

/// Parses a stored UUID string into a [`NodeId`].
fn parse_node_id(s: &str) -> Result<NodeId, MemoryError> {
    s.parse::<uuid::Uuid>()
        .map(NodeId::from_uuid)
        .map_err(|_| MemoryError::Backend(format!("corrupt node id: {s}")))
}

/// Returns `true` if a node with the given id string exists.
fn node_exists(conn: &Connection, id_str: &str) -> Result<bool, MemoryError> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM nodes WHERE id = ?1", [id_str], |row| {
            row.get(0)
        })
        .optional()
        .map_err(db_err)?;
    Ok(found.is_some())
}

/// Loads a node by id string, returning its [`NodeId`] and [`Fact`].
fn load_node(conn: &Connection, id_str: &str) -> Result<Option<(NodeId, Fact)>, MemoryError> {
    let row = conn
        .query_row(
            "SELECT id, kind, label, props FROM nodes WHERE id = ?1",
            [id_str],
            row_to_node,
        )
        .optional()
        .map_err(db_err)?;
    row.transpose()
}

/// Re-synchronizes the FTS index for a node after its properties change.
fn sync_fts(
    conn: &Connection,
    id_str: &str,
    label: &str,
    props_str: &str,
) -> Result<(), MemoryError> {
    conn.execute("DELETE FROM nodes_fts WHERE node_id = ?1", [id_str])
        .map_err(db_err)?;
    conn.execute(
        "INSERT INTO nodes_fts (node_id, label, props) VALUES (?1, ?2, ?3)",
        rusqlite::params![id_str, label, props_str],
    )
    .map_err(db_err)?;
    Ok(())
}

/// Returns `true` if `fact.kind` matches the optional kind filter.
fn kind_matches(fact: &Fact, kind: Option<&str>) -> bool {
    kind.is_none_or(|k| fact.kind == k)
}

/// Builds an FTS5 phrase MATCH expression that safely quotes arbitrary text.
fn fts_phrase(text: &str) -> String {
    let escaped = text.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

/// Maps a `nodes` row into a `(NodeId, Fact)` result, deferring JSON/UUID
/// parse errors so the outer query can surface them.
fn row_to_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<(NodeId, Fact), MemoryError>> {
    let id_str: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let label: String = row.get(2)?;
    let props_str: String = row.get(3)?;
    Ok((|| {
        let id = parse_node_id(&id_str)?;
        let props: serde_json::Value = serde_json::from_str(&props_str)?;
        Ok((id, Fact::new(kind, label, props)))
    })())
}

/// Collects mapped `nodes` rows into [`MemoryHit`]s with a uniform score.
fn rows_to_hits(
    rows: rusqlite::MappedRows<
        '_,
        impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Result<(NodeId, Fact), MemoryError>>,
    >,
) -> Result<Vec<MemoryHit>, MemoryError> {
    let mut hits = Vec::new();
    for row in rows {
        let (id, fact) = row.map_err(db_err)??;
        hits.push(MemoryHit::new(id, fact, 1.0));
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph() -> MemoryGraph {
        MemoryGraph::in_memory().unwrap()
    }

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }

    #[tokio::test]
    async fn upsert_dedups_on_natural_key_and_updates_props() {
        let g = graph();
        let id1 = g
            .upsert_fact(Fact::new("person", "Alice", json!({"role": "dev"})))
            .await
            .unwrap();
        // Same (kind, label) → same id, no duplicate node.
        let id2 = g
            .upsert_fact(Fact::new("person", "Alice", json!({"role": "lead"})))
            .await
            .unwrap();
        assert_eq!(id1, id2);

        // The updated props are persisted (latest write wins).
        let hits = g
            .query(MemoryQuery::new().with_kind("person"))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fact.props["role"], json!("lead"));
    }

    #[tokio::test]
    async fn link_and_traverse_returns_linked_node() {
        let g = graph();
        let alice = g
            .upsert_fact(Fact::new("person", "Alice", json!({})))
            .await
            .unwrap();
        let issue = g
            .upsert_fact(Fact::new("issue", "ISSUE-1", json!({"title": "bug"})))
            .await
            .unwrap();

        g.link(alice, Relation::new("assigned_to"), issue)
            .await
            .unwrap();

        let hits = g
            .query(MemoryQuery::new().traversing(alice, Relation::new("assigned_to")))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, issue);
        assert_eq!(hits[0].fact.label, "ISSUE-1");
    }

    #[tokio::test]
    async fn traversal_respects_kind_filter() {
        let g = graph();
        let work = g
            .upsert_fact(Fact::new("task", "Ship release", json!({})))
            .await
            .unwrap();
        let file = g
            .upsert_fact(Fact::new("file", "main.rs", json!({})))
            .await
            .unwrap();
        let person = g
            .upsert_fact(Fact::new("person", "Bob", json!({})))
            .await
            .unwrap();

        g.link(work, Relation::new("involves"), file).await.unwrap();
        g.link(work, Relation::new("involves"), person)
            .await
            .unwrap();

        let files = g
            .query(
                MemoryQuery::new()
                    .traversing(work, Relation::new("involves"))
                    .with_kind("file"),
            )
            .await
            .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, file);
    }

    #[tokio::test]
    async fn link_missing_endpoint_returns_node_not_found() {
        let g = graph();
        let real = g
            .upsert_fact(Fact::new("person", "Carol", json!({})))
            .await
            .unwrap();
        let ghost = NodeId::new();

        let err = g
            .link(real, Relation::new("knows"), ghost)
            .await
            .unwrap_err();
        match err {
            MemoryError::NodeNotFound(id) => assert_eq!(id, ghost),
            other => panic!("expected NodeNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn text_query_matches_label_and_props() {
        let g = graph();
        g.upsert_fact(Fact::new(
            "file",
            "auth.rs",
            json!({"summary": "handles login"}),
        ))
        .await
        .unwrap();
        g.upsert_fact(Fact::new(
            "file",
            "render.rs",
            json!({"summary": "draws ui"}),
        ))
        .await
        .unwrap();

        // Match by label token.
        let by_label = g.query(MemoryQuery::new().with_text("auth")).await.unwrap();
        assert_eq!(by_label.len(), 1);
        assert_eq!(by_label[0].fact.label, "auth.rs");

        // Match by a token inside props.
        let by_props = g
            .query(MemoryQuery::new().with_text("login"))
            .await
            .unwrap();
        assert_eq!(by_props.len(), 1);
        assert_eq!(by_props[0].fact.label, "auth.rs");
    }

    #[tokio::test]
    async fn text_query_with_kind_filter_narrows_results() {
        let g = graph();
        g.upsert_fact(Fact::new("file", "report", json!({})))
            .await
            .unwrap();
        g.upsert_fact(Fact::new("issue", "report", json!({})))
            .await
            .unwrap();

        let hits = g
            .query(MemoryQuery::new().with_text("report").with_kind("issue"))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fact.kind, "issue");
    }

    #[tokio::test]
    async fn limit_caps_results() {
        let g = graph();
        for i in 0..5 {
            g.upsert_fact(Fact::new("file", format!("f{i}.rs"), json!({})))
                .await
                .unwrap();
        }
        let hits = g
            .query(MemoryQuery::new().with_kind("file").with_limit(2))
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[tokio::test]
    async fn duplicate_link_is_idempotent() {
        let g = graph();
        let a = g
            .upsert_fact(Fact::new("person", "A", json!({})))
            .await
            .unwrap();
        let b = g
            .upsert_fact(Fact::new("person", "B", json!({})))
            .await
            .unwrap();
        g.link(a, Relation::new("knows"), b).await.unwrap();
        g.link(a, Relation::new("knows"), b).await.unwrap();

        let hits = g
            .query(MemoryQuery::new().traversing(a, Relation::new("knows")))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn invalid_traversal_query_is_rejected() {
        let g = graph();
        let anchor = g.upsert_fact(Fact::new("x", "y", json!({}))).await.unwrap();
        let mut q = MemoryQuery::new();
        q.from = Some(anchor); // relation intentionally omitted
        let err = g.query(q).await.unwrap_err();
        assert!(matches!(err, MemoryError::InvalidQuery(_)));
    }

    #[tokio::test]
    async fn entity_link_helper_links_file_to_work() {
        let g = graph();
        let work = g
            .upsert_fact(Fact::new("task", "Refactor parser", json!({})))
            .await
            .unwrap();

        let file_id = g
            .link_entity_to_work(
                Fact::new("file", "parser.rs", json!({"loc": 420})),
                Relation::new("appears_in"),
                work,
            )
            .await
            .unwrap();

        // The file is retrievable by traversing the work it appears in.
        let appears = g
            .query(MemoryQuery::new().traversing(file_id, Relation::new("appears_in")))
            .await
            .unwrap();
        assert_eq!(appears.len(), 1);
        assert_eq!(appears[0].id, work);
    }

    #[test]
    fn file_backed_graph_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.db");

        let id = {
            let g = MemoryGraph::open(&path).unwrap();
            // Block on the async upsert using a tiny runtime.
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                g.upsert_fact(Fact::new("person", "Dana", json!({"team": "core"})))
                    .await
                    .unwrap()
            })
        };

        // Reopen the same file and confirm the node is still present.
        let g2 = MemoryGraph::open(&path).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let hits = rt.block_on(async {
            g2.query(MemoryQuery::new().with_kind("person"))
                .await
                .unwrap()
        });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
        assert_eq!(hits[0].fact.props["team"], json!("core"));
    }
}
