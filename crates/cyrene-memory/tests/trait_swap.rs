//! Integration test for Memory backend swap via config (R16.5).
//!
//! The `Memory` trait is registered in the `PluginRegistry` by `type`, so a
//! Maintainer can swap the memory backend purely through the TOML config
//! without any change to the core engine. This test wires the real
//! [`MemoryGraph`] in as the `sqlite` memory type, loads it through the
//! registry from a config string, and exercises it through an
//! `Arc<dyn Memory>` trait object — proving the backend is genuinely swappable
//! behind the trait.

use std::sync::Arc;

use cyrene_config::{Config, PluginRegistry, SecretResolver};
use cyrene_core::{Fact, Memory, MemoryQuery, Relation};
use cyrene_memory::MemoryGraph;
use serde_json::json;

/// A minimal valid config: one provider, one channel (required by validation),
/// plus a `[memory.sqlite.default]` backend declaration.
const CONFIG: &str = r#"
[providers.openai.coding]
api_key_env = "OPENAI_API_KEY"

[channels.cli.default]

[memory.sqlite.default]
path = ":memory:"
"#;

#[test]
fn memory_backend_is_swappable_through_the_registry() {
    let cfg = Config::parse(CONFIG, "test.toml").unwrap();
    let secrets = SecretResolver::from_env();

    let mut registry = PluginRegistry::new();

    // Register the real SQLite-backed MemoryGraph as the `sqlite` memory type.
    // A different deployment could register an entirely different backend here
    // under the same or a different `type` without touching the core engine.
    registry.register_memory("sqlite", |_ctx| {
        let graph = MemoryGraph::in_memory()?;
        Ok(Arc::new(graph) as Arc<dyn Memory>)
    });

    // Loading builds every component that has a registered factory. The
    // provider/channel have no factory here and are reported as load failures,
    // but the memory backend builds successfully — which is all this test cares
    // about.
    let _failures = registry.load(&cfg, &secrets);

    // The memory backend is looked up by its `type`/`alias` and used purely
    // through the `Memory` trait object.
    let mem: Arc<dyn Memory> = registry
        .memory("sqlite", "default")
        .expect("sqlite memory backend should be registered and built");

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    rt.block_on(async {
        // upsert + dedup behavior is exercised through the trait.
        let person = mem
            .upsert_fact(Fact::new("person", "Eve", json!({"role": "ops"})))
            .await
            .unwrap();
        let issue = mem
            .upsert_fact(Fact::new("issue", "OPS-7", json!({})))
            .await
            .unwrap();

        // Re-upsert same natural key returns the same id (no duplicate).
        let person_again = mem
            .upsert_fact(Fact::new("person", "Eve", json!({"role": "sre"})))
            .await
            .unwrap();
        assert_eq!(person, person_again);

        // link + traverse through the trait object.
        mem.link(person, Relation::new("handles"), issue)
            .await
            .unwrap();
        let hits = mem
            .query(MemoryQuery::new().traversing(person, Relation::new("handles")))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, issue);
    });

    assert_eq!(registry.memory_count(), 1);
}
