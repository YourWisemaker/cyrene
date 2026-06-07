//! Integration test for the opt-in Redis Memory backend wiring (R16.5).
//!
//! Compiled only with `--features redis`. It proves the same swap path the
//! SQLite backend uses also works for Redis: a `[memory.redis.<alias>]` entry
//! flows through the `PluginRegistry`, its `url_env` secret is resolved, and a
//! `RedisMemory` is built behind an `Arc<dyn Memory>` — all without a live
//! server, because `RedisMemory::new` connects lazily on first use.
//!
//! Exercising actual reads/writes needs a server and lives in the `--ignored`
//! test inside `src/redis.rs`.

#![cfg(feature = "redis")]

use std::sync::Arc;

use cyrene_config::{Config, PluginRegistry, SecretResolver};
use cyrene_core::Memory;
use cyrene_memory::RedisMemory;

/// Minimal valid config plus a Redis memory backend referencing a URL secret.
const CONFIG: &str = r#"
[providers.openai.coding]
api_key_env = "OPENAI_API_KEY"

[channels.cli.default]

[memory.redis.default]
url_env = "CYRENE_TEST_REDIS_URL"
"#;

#[test]
fn redis_backend_is_buildable_through_the_registry() {
    // The URL secret is resolved by name, never stored in the config.
    std::env::set_var("CYRENE_TEST_REDIS_URL", "redis://127.0.0.1/");

    let cfg = Config::parse(CONFIG, "test.toml").unwrap();
    let secrets = SecretResolver::from_env();

    let mut registry = PluginRegistry::new();

    // A deployment opts into Redis simply by registering this factory; the
    // factory is synchronous because `RedisMemory::new` defers the connection.
    registry.register_memory("redis", |ctx| {
        let env = ctx
            .entry
            .url_env
            .as_deref()
            .ok_or("memory.redis requires `url_env`")?;
        let url = ctx.secrets.require(env)?;
        Ok(Arc::new(RedisMemory::new(&url)?) as Arc<dyn Memory>)
    });

    let _failures = registry.load(&cfg, &secrets);

    let mem = registry
        .memory("redis", "default")
        .expect("redis memory backend should be registered and built");

    // It is a usable `Memory` trait object even though no server is connected.
    assert_eq!(registry.memory_count(), 1);
    let _: &dyn Memory = mem.as_ref();
}
