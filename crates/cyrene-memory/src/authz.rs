//! Principal-trust boundary for memory: only the owner may manipulate it (R16).
//!
//! [`crate::guard::GuardedMemory`] answers "is this *content* safe to store?".
//! This module answers a different question: "is this *principal* allowed to
//! touch memory at all?". The two are orthogonal — a guard stops a poisoned web
//! page; an authorization boundary stops a hijacked or spoofed session.
//!
//! ## Threat model
//!
//! Cyrene's memory is personal: it belongs to the one authenticated user who
//! owns the instance, whether they reach it from Telegram, a local shell, or
//! inside a VPS. An attacker who manages to drive a session — a stolen Telegram
//! account, a spoofed `user_id`, a replayed request — must **not** be able to
//! read or rewrite that memory.
//!
//! [`AuthorizedMemory`] enforces that by binding a wrapped [`Memory`] backend to
//! an owner [`UserId`] and exposing **only** principal-aware operations. Every
//! call must present the principal making the request; if it is not the owner,
//! the operation is refused with [`MemoryError::Unauthorized`] and the backend
//! is never touched. Each stored fact is additionally stamped with its owner, so
//! a read filters out anything not owned by the caller — defending against rows
//! inserted out-of-band (e.g. direct DB tampering) that lack a valid owner
//! stamp.
//!
//! ## Deliberately not a `Memory`
//!
//! `AuthorizedMemory` does **not** implement the [`Memory`] trait. The trait's
//! methods carry no principal, so exposing them would reintroduce an
//! unauthenticated write path — the exact hole this type closes. Callers must go
//! through the explicit `*_as` methods, which makes "who is asking?" impossible
//! to forget.
//!
//! ## Composition
//!
//! Principal-trust and content-trust protect different ingress paths and are
//! applied independently:
//!
//! - The **owner's** authenticated writes go through [`AuthorizedMemory`]. The
//!   owner is trusted, so their content is not injection-scanned.
//! - Content Cyrene **ingests from the outside** (web pages, tool output) goes
//!   through [`GuardedMemory::upsert_fact_from`](crate::GuardedMemory) on the
//!   ingest path, where it is scanned and tagged untrusted.

use serde_json::{Map, Value};

use cyrene_core::{Fact, Memory, MemoryError, MemoryHit, MemoryQuery, NodeId, Relation, UserId};

/// Reserved property key recording the [`UserId`] that owns a fact.
pub const OWNER_KEY: &str = "__cyrene_owner";

/// Reserved property key under which non-object properties are nested so the
/// owner stamp can always live alongside them in an object.
pub const VALUE_KEY: &str = "__cyrene_value";

/// A principal-gated facade over any [`Memory`] backend.
///
/// Bind a backend to its `owner`, then drive it through the `*_as` methods. A
/// request from any principal other than the owner is refused before the
/// backend is touched.
#[derive(Debug)]
pub struct AuthorizedMemory<M: Memory> {
    inner: M,
    owner: UserId,
}

impl<M: Memory> AuthorizedMemory<M> {
    /// Binds `inner` to its `owner`. Only `owner` may subsequently manipulate it.
    #[must_use]
    pub fn new(inner: M, owner: UserId) -> Self {
        Self { inner, owner }
    }

    /// The user this memory belongs to.
    #[must_use]
    pub fn owner(&self) -> &UserId {
        &self.owner
    }

    /// Borrows the wrapped backend.
    pub fn inner(&self) -> &M {
        &self.inner
    }

    /// Returns `Ok(())` only if `principal` is the owner.
    fn authorize(&self, principal: &UserId) -> Result<(), MemoryError> {
        if principal == &self.owner {
            Ok(())
        } else {
            // Name only the rejected principal, never the owner, so the error is
            // safe to log without leaking who the legitimate owner is.
            Err(MemoryError::Unauthorized(format!(
                "principal {principal} may not manipulate memory owned by another user"
            )))
        }
    }

    /// Upserts a fact on behalf of `principal`.
    ///
    /// # Errors
    /// Returns [`MemoryError::Unauthorized`] if `principal` is not the owner, or
    /// any [`MemoryError`] the backend raises.
    pub async fn upsert_fact_as(
        &self,
        fact: Fact,
        principal: &UserId,
    ) -> Result<NodeId, MemoryError> {
        self.authorize(principal)?;
        self.inner.upsert_fact(stamp_owner(fact, &self.owner)).await
    }

    /// Queries memory on behalf of `principal`, returning only facts the owner
    /// actually owns.
    ///
    /// # Errors
    /// Returns [`MemoryError::Unauthorized`] if `principal` is not the owner, or
    /// any [`MemoryError`] the backend raises.
    pub async fn query_as(
        &self,
        q: MemoryQuery,
        principal: &UserId,
    ) -> Result<Vec<MemoryHit>, MemoryError> {
        self.authorize(principal)?;
        let hits = self.inner.query(q).await?;
        Ok(hits
            .into_iter()
            .filter(|hit| owner_matches(&hit.fact, &self.owner))
            .collect())
    }

    /// Creates a relationship on behalf of `principal`.
    ///
    /// # Errors
    /// Returns [`MemoryError::Unauthorized`] if `principal` is not the owner, or
    /// any [`MemoryError`] the backend raises.
    pub async fn link_as(
        &self,
        from: NodeId,
        rel: Relation,
        to: NodeId,
        principal: &UserId,
    ) -> Result<(), MemoryError> {
        self.authorize(principal)?;
        self.inner.link(from, rel, to).await
    }
}

/// Returns the [`UserId`] stamped on a fact, if any.
#[must_use]
pub fn owner_of(fact: &Fact) -> Option<UserId> {
    fact.props
        .get(OWNER_KEY)
        .and_then(Value::as_str)
        .map(UserId::new)
}

// ─── internal helpers ────────────────────────────────────────────────────────

/// Returns a copy of `fact` whose properties carry the owner stamp. Non-object
/// properties are nested under [`VALUE_KEY`] so the stamp can sit beside them.
fn stamp_owner(fact: Fact, owner: &UserId) -> Fact {
    let mut map = match fact.props {
        Value::Object(map) => map,
        other => {
            let mut m = Map::new();
            m.insert(VALUE_KEY.to_owned(), other);
            m
        }
    };
    map.insert(
        OWNER_KEY.to_owned(),
        Value::String(owner.as_str().to_owned()),
    );
    Fact::new(fact.kind, fact.label, Value::Object(map))
}

/// Returns `true` if a recalled fact is stamped as owned by `owner`. A fact with
/// no owner stamp is treated as not owned (fail closed), so out-of-band rows are
/// withheld.
fn owner_matches(fact: &Fact, owner: &UserId) -> bool {
    owner_of(fact).as_ref() == Some(owner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryGraph;
    use serde_json::json;

    fn owned() -> AuthorizedMemory<MemoryGraph> {
        AuthorizedMemory::new(MemoryGraph::in_memory().unwrap(), UserId::new("alice"))
    }

    // ── Owner may read and write ─────────────────────────────────────────────

    #[tokio::test]
    async fn owner_can_write_and_read() {
        let m = owned();
        let alice = UserId::new("alice");
        m.upsert_fact_as(Fact::new("pref", "dark mode", json!({})), &alice)
            .await
            .unwrap();

        let hits = m.query_as(MemoryQuery::new(), &alice).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(owner_of(&hits[0].fact), Some(alice));
    }

    // ── A non-owner (hijacked / spoofed session) is refused ──────────────────

    #[tokio::test]
    async fn non_owner_write_is_unauthorized() {
        let m = owned();
        let attacker = UserId::new("mallory");
        let err = m
            .upsert_fact_as(Fact::new("pref", "evil", json!({})), &attacker)
            .await
            .unwrap_err();
        assert!(matches!(err, MemoryError::Unauthorized(_)));

        // Nothing was written: the owner sees an empty graph.
        let hits = m
            .query_as(MemoryQuery::new(), &UserId::new("alice"))
            .await
            .unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn non_owner_read_is_unauthorized() {
        let m = owned();
        let err = m
            .query_as(MemoryQuery::new(), &UserId::new("mallory"))
            .await
            .unwrap_err();
        assert!(matches!(err, MemoryError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn non_owner_link_is_unauthorized() {
        let m = owned();
        let alice = UserId::new("alice");
        let a = m
            .upsert_fact_as(Fact::new("x", "a", json!({})), &alice)
            .await
            .unwrap();
        let b = m
            .upsert_fact_as(Fact::new("x", "b", json!({})), &alice)
            .await
            .unwrap();

        let err = m
            .link_as(a, Relation::new("knows"), b, &UserId::new("mallory"))
            .await
            .unwrap_err();
        assert!(matches!(err, MemoryError::Unauthorized(_)));
    }

    // ── Error names the rejected principal, not the owner ────────────────────

    #[tokio::test]
    async fn unauthorized_error_does_not_leak_owner() {
        let m = owned();
        let err = m
            .upsert_fact_as(Fact::new("x", "y", json!({})), &UserId::new("mallory"))
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mallory"));
        assert!(!msg.contains("alice"));
    }

    // ── Reads fail closed for unstamped / foreign-owned rows ─────────────────

    #[tokio::test]
    async fn query_withholds_facts_not_owned_by_caller() {
        // Insert rows directly through the backend: one stamped for a different
        // owner, one with no owner stamp at all (e.g. out-of-band DB write).
        let inner = MemoryGraph::in_memory().unwrap();
        inner
            .upsert_fact(Fact::new(
                "pref",
                "foreign",
                json!({ OWNER_KEY: "mallory" }),
            ))
            .await
            .unwrap();
        inner
            .upsert_fact(Fact::new("pref", "unstamped", json!({})))
            .await
            .unwrap();
        // And one genuinely owned by alice.
        inner
            .upsert_fact(Fact::new("pref", "mine", json!({ OWNER_KEY: "alice" })))
            .await
            .unwrap();

        let m = AuthorizedMemory::new(inner, UserId::new("alice"));
        let hits = m
            .query_as(MemoryQuery::new(), &UserId::new("alice"))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fact.label, "mine");
    }

    // ── Non-object props are preserved under the value key ───────────────────

    #[tokio::test]
    async fn non_object_props_are_nested_and_owned() {
        let m = owned();
        let alice = UserId::new("alice");
        m.upsert_fact_as(Fact::new("scalar", "answer", json!(42)), &alice)
            .await
            .unwrap();

        let hits = m.query_as(MemoryQuery::new(), &alice).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fact.props[VALUE_KEY], json!(42));
        assert_eq!(owner_of(&hits[0].fact), Some(alice));
    }
}
