//! The `Plugin_Registry` (task 3.2).
//!
//! The registry turns a loaded [`Config`] into live components. It instantiates
//! every declared Channel, Memory backend, and Model_Provider by its
//! `type`/`alias`, inserts each into an alias-keyed lookup table, and — when a
//! component fails to initialize — **skips it, collects the error, and keeps
//! going** so the runtime can continue with the rest (R2.4). The collected
//! failures are surfaced to the caller (the runtime) to log to the
//! Receipt_Ledger.
//!
//! ## Factory abstraction (R2.2)
//!
//! Concrete providers and channels live in other crates (`cyrene-models`,
//! `cyrene-channels`, …) that are built later, so this crate must **not** depend
//! on them. Instead, the registry holds a `type -> factory` map per kind. A
//! factory is a closure that, given a [`BuildContext`] (the config entry plus a
//! [`SecretResolver`] and the component's `type`/`alias`), produces an
//! `Arc<dyn Channel>` / `Arc<dyn Memory>` / `Arc<dyn Model>`. Whoever wires the
//! runtime registers a factory for each component `type`; new component types
//! register from the outside with no change to `cyrene-config` or the core
//! engine (R2.2).
//!
//! ```no_run
//! use std::sync::Arc;
//! use cyrene_config::{Config, PluginRegistry, SecretResolver};
//! # use cyrene_core::{Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Tier};
//! # use async_trait::async_trait;
//! # struct MyProvider(ModelDescriptor);
//! # #[async_trait]
//! # impl Model for MyProvider {
//! #     fn descriptor(&self) -> ModelDescriptor { self.0.clone() }
//! #     async fn complete(&self, _: ModelRequest) -> Result<ModelResponse, ModelError> { unimplemented!() }
//! #     async fn embed(&self, _: &str) -> Result<Vec<f32>, ModelError> { unimplemented!() }
//! # }
//! let mut registry = PluginRegistry::new();
//! registry.register_model("openai", |ctx| {
//!     // Resolve the API key by env-var NAME; the value never lives in the TOML.
//!     let _key = ctx
//!         .entry
//!         .api_key_env
//!         .as_deref()
//!         .map(|name| ctx.secrets.require(name))
//!         .transpose()?;
//!     Ok(Arc::new(MyProvider(ModelDescriptor {
//!         alias: ctx.alias.to_owned(),
//!         tier: ctx.entry.tier.unwrap_or(Tier::Local),
//!         input_price: cyrene_core::Money::new("USD", 0),
//!         output_price: cyrene_core::Money::new("USD", 0),
//!     })) as Arc<dyn Model>)
//! });
//!
//! let config = Config::load()?;
//! let secrets = SecretResolver::with_dotenv();
//! let failures = registry.load(&config, &secrets);
//! for failure in failures {
//!     eprintln!("skipped {failure}"); // the runtime logs these to the ledger
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;

use cyrene_core::{Channel, Memory, Model, Recoverability, Recoverable};

use crate::config::{ChannelEntry, Config, MemoryEntry, ProviderEntry};
use crate::secrets::SecretResolver;

/// A boxed, thread-safe error returned by a component factory.
///
/// Factories live in other crates and return their own concrete error types
/// (e.g. `ModelError`, or a [`ConfigError`](crate::ConfigError) for a missing
/// secret); boxing lets the registry collect any of them uniformly into a
/// [`LoadFailure`] without depending on the concrete provider crates.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// The context handed to a component factory when the registry builds one
/// configured instance.
///
/// Carries the component's coordinates (`type_name`/`alias`), its deserialized
/// config `entry`, and the [`SecretResolver`] so the factory can resolve any
/// referenced secret by environment-variable name. The factory borrows
/// everything for the duration of the call and returns an owned `Arc<dyn …>`.
#[derive(Debug)]
pub struct BuildContext<'a, T> {
    /// The component type (the first TOML key, e.g. `"openai"`).
    pub type_name: &'a str,
    /// The component alias (the second TOML key, e.g. `"coding"`).
    pub alias: &'a str,
    /// The deserialized config entry for this instance.
    pub entry: &'a T,
    /// The resolver used to read referenced secrets from the environment.
    pub secrets: &'a SecretResolver,
}

/// Which kind of component a [`LoadFailure`] concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    /// A [`Channel`] declared under `[channels.<type>.<alias>]`.
    Channel,
    /// A [`Model`] provider declared under `[providers.<type>.<alias>]`.
    Model,
    /// A [`Memory`] backend declared under `[memory.<type>.<alias>]`.
    Memory,
}

impl ComponentKind {
    /// Returns the lowercase noun for this kind, used in diagnostics.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Channel => "channel",
            Self::Model => "model",
            Self::Memory => "memory",
        }
    }
}

impl core::fmt::Display for ComponentKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The composite alias key a built component is stored under: its `type` plus
/// its `alias` (e.g. type `"openai"`, alias `"coding"`).
///
/// Keys are scoped per kind, so a Channel and a Model may share a
/// `type`/`alias` without colliding. [`Display`](core::fmt::Display) renders it
/// as `type.alias`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentKey {
    /// The component type (e.g. `"openai"`).
    pub type_name: String,
    /// The component alias (e.g. `"coding"`).
    pub alias: String,
}

impl ComponentKey {
    /// Creates a key from a `type` and `alias`.
    pub fn new(type_name: impl Into<String>, alias: impl Into<String>) -> Self {
        Self {
            type_name: type_name.into(),
            alias: alias.into(),
        }
    }
}

impl core::fmt::Display for ComponentKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}", self.type_name, self.alias)
    }
}

/// Why a single configured component could not be instantiated.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// No factory was registered for the component's `type`.
    ///
    /// The config references a `type` (e.g. an extension or provider) that no
    /// one registered a factory for, so the registry cannot build it.
    #[error("no factory registered for component type `{0}`")]
    NoFactory(String),

    /// The factory ran but failed to initialize the component (e.g. a missing
    /// secret, an unreachable endpoint, or invalid settings).
    #[error("component failed to initialize: {0}")]
    Init(#[source] BoxError),
}

impl Recoverable for LoadError {
    fn recoverability(&self) -> Recoverability {
        // Both cases point at the user's config/environment: register/install
        // the missing component type, or fix the setting/secret that made init
        // fail. Neither is fixed by an automatic retry.
        Recoverability::UserAction
    }
}

/// A component the registry skipped because it failed to load (R2.4).
///
/// The runtime logs each of these to the Receipt_Ledger and continues with the
/// components that did load.
#[derive(Debug)]
pub struct LoadFailure {
    /// Which kind of component failed.
    pub kind: ComponentKind,
    /// The `type`/`alias` of the component that failed.
    pub key: ComponentKey,
    /// Why it failed.
    pub error: LoadError,
}

impl LoadFailure {
    /// Creates a load failure record.
    #[must_use]
    pub fn new(kind: ComponentKind, key: ComponentKey, error: LoadError) -> Self {
        Self { kind, key, error }
    }
}

impl core::fmt::Display for LoadFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} `{}`: {}", self.kind, self.key, self.error)
    }
}

/// A factory that builds one [`Channel`] instance from its config entry.
type ChannelFactory =
    Arc<dyn Fn(BuildContext<'_, ChannelEntry>) -> Result<Arc<dyn Channel>, BoxError> + Send + Sync>;
/// A factory that builds one [`Model`] instance from its config entry.
type ModelFactory =
    Arc<dyn Fn(BuildContext<'_, ProviderEntry>) -> Result<Arc<dyn Model>, BoxError> + Send + Sync>;
/// A factory that builds one [`Memory`] instance from its config entry.
type MemoryFactory =
    Arc<dyn Fn(BuildContext<'_, MemoryEntry>) -> Result<Arc<dyn Memory>, BoxError> + Send + Sync>;

/// The Plugin_Registry: builds and holds the live components for a [`Config`].
///
/// Usage is two-phase:
/// 1. Register a factory for each component `type` you support
///    ([`register_channel`](Self::register_channel),
///    [`register_model`](Self::register_model),
///    [`register_memory`](Self::register_memory)).
/// 2. Call [`load`](Self::load) with the loaded [`Config`] and a
///    [`SecretResolver`]; the registry instantiates every declared, enabled
///    component, populating the alias-keyed lookup tables and collecting any
///    [`LoadFailure`]s.
///
/// After loading, look components up by `type`/`alias`
/// ([`channel`](Self::channel), [`model`](Self::model), [`memory`](Self::memory))
/// or list a whole kind ([`channels`](Self::channels), [`models`](Self::models),
/// [`memories`](Self::memories)). Model **selection** is not done here — the
/// registry exposes all registered models and the Model_Router chooses among
/// them (R2.3).
///
/// [`load`](Self::load) is idempotent: each call clears the previously built
/// instances and failures, then rebuilds from the supplied config, so the
/// registry can be reloaded after a config change while keeping its registered
/// factories.
#[derive(Default)]
pub struct PluginRegistry {
    channel_factories: BTreeMap<String, ChannelFactory>,
    model_factories: BTreeMap<String, ModelFactory>,
    memory_factories: BTreeMap<String, MemoryFactory>,

    channels: BTreeMap<ComponentKey, Arc<dyn Channel>>,
    models: BTreeMap<ComponentKey, Arc<dyn Model>>,
    memories: BTreeMap<ComponentKey, Arc<dyn Memory>>,

    failures: Vec<LoadFailure>,
}

impl PluginRegistry {
    /// Creates an empty registry with no factories and no built components.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a factory that builds a [`Channel`] for the given component
    /// `type` (e.g. `"telegram"`). Re-registering a `type` replaces the prior
    /// factory.
    pub fn register_channel<F>(&mut self, type_name: impl Into<String>, factory: F)
    where
        F: Fn(BuildContext<'_, ChannelEntry>) -> Result<Arc<dyn Channel>, BoxError>
            + Send
            + Sync
            + 'static,
    {
        self.channel_factories
            .insert(type_name.into(), Arc::new(factory));
    }

    /// Registers a factory that builds a [`Model`] provider for the given
    /// component `type` (e.g. `"openai"`). Re-registering a `type` replaces the
    /// prior factory.
    pub fn register_model<F>(&mut self, type_name: impl Into<String>, factory: F)
    where
        F: Fn(BuildContext<'_, ProviderEntry>) -> Result<Arc<dyn Model>, BoxError>
            + Send
            + Sync
            + 'static,
    {
        self.model_factories
            .insert(type_name.into(), Arc::new(factory));
    }

    /// Registers a factory that builds a [`Memory`] backend for the given
    /// component `type` (e.g. `"sqlite"`). Re-registering a `type` replaces the
    /// prior factory.
    pub fn register_memory<F>(&mut self, type_name: impl Into<String>, factory: F)
    where
        F: Fn(BuildContext<'_, MemoryEntry>) -> Result<Arc<dyn Memory>, BoxError>
            + Send
            + Sync
            + 'static,
    {
        self.memory_factories
            .insert(type_name.into(), Arc::new(factory));
    }

    /// Returns `true` if a factory is registered for the given channel `type`.
    #[must_use]
    pub fn has_channel_factory(&self, type_name: &str) -> bool {
        self.channel_factories.contains_key(type_name)
    }

    /// Returns `true` if a factory is registered for the given model `type`.
    #[must_use]
    pub fn has_model_factory(&self, type_name: &str) -> bool {
        self.model_factories.contains_key(type_name)
    }

    /// Returns `true` if a factory is registered for the given memory `type`.
    #[must_use]
    pub fn has_memory_factory(&self, type_name: &str) -> bool {
        self.memory_factories.contains_key(type_name)
    }

    /// Instantiates every declared, enabled component in `config`, populating
    /// the lookup tables and collecting any [`LoadFailure`]s.
    ///
    /// On a component init failure — the factory returned `Err`, or no factory
    /// is registered for the component's `type` — the registry **skips** that
    /// component, records a [`LoadFailure`], and continues with the rest
    /// (R2.4). Disabled entries (`enabled = false`) are skipped silently and
    /// are never treated as failures.
    ///
    /// Returns the failures collected during this load so the caller can log
    /// them to the Receipt_Ledger. The same list is also available afterward
    /// via [`failures`](Self::failures).
    pub fn load(&mut self, config: &Config, secrets: &SecretResolver) -> &[LoadFailure] {
        self.channels.clear();
        self.models.clear();
        self.memories.clear();
        self.failures.clear();

        // Channels.
        for c in config.channels() {
            if !c.entry.enabled {
                continue;
            }
            let key = ComponentKey::new(c.type_name, c.alias);
            let Some(factory) = self.channel_factories.get(c.type_name).cloned() else {
                self.failures.push(LoadFailure::new(
                    ComponentKind::Channel,
                    key,
                    LoadError::NoFactory(c.type_name.to_owned()),
                ));
                continue;
            };
            let ctx = BuildContext {
                type_name: c.type_name,
                alias: c.alias,
                entry: c.entry,
                secrets,
            };
            match (*factory)(ctx) {
                Ok(instance) => {
                    self.channels.insert(key, instance);
                }
                Err(err) => self.failures.push(LoadFailure::new(
                    ComponentKind::Channel,
                    key,
                    LoadError::Init(err),
                )),
            }
        }

        // Model providers.
        for p in config.providers() {
            if !p.entry.enabled {
                continue;
            }
            let key = ComponentKey::new(p.type_name, p.alias);
            let Some(factory) = self.model_factories.get(p.type_name).cloned() else {
                self.failures.push(LoadFailure::new(
                    ComponentKind::Model,
                    key,
                    LoadError::NoFactory(p.type_name.to_owned()),
                ));
                continue;
            };
            let ctx = BuildContext {
                type_name: p.type_name,
                alias: p.alias,
                entry: p.entry,
                secrets,
            };
            match (*factory)(ctx) {
                Ok(instance) => {
                    self.models.insert(key, instance);
                }
                Err(err) => self.failures.push(LoadFailure::new(
                    ComponentKind::Model,
                    key,
                    LoadError::Init(err),
                )),
            }
        }

        // Memory backends.
        for m in config.memory() {
            if !m.entry.enabled {
                continue;
            }
            let key = ComponentKey::new(m.type_name, m.alias);
            let Some(factory) = self.memory_factories.get(m.type_name).cloned() else {
                self.failures.push(LoadFailure::new(
                    ComponentKind::Memory,
                    key,
                    LoadError::NoFactory(m.type_name.to_owned()),
                ));
                continue;
            };
            let ctx = BuildContext {
                type_name: m.type_name,
                alias: m.alias,
                entry: m.entry,
                secrets,
            };
            match (*factory)(ctx) {
                Ok(instance) => {
                    self.memories.insert(key, instance);
                }
                Err(err) => self.failures.push(LoadFailure::new(
                    ComponentKind::Memory,
                    key,
                    LoadError::Init(err),
                )),
            }
        }

        &self.failures
    }

    /// Looks up a built [`Channel`] by `type`/`alias`.
    #[must_use]
    pub fn channel(&self, type_name: &str, alias: &str) -> Option<Arc<dyn Channel>> {
        self.channels
            .get(&ComponentKey::new(type_name, alias))
            .cloned()
    }

    /// Looks up a built [`Model`] provider by `type`/`alias`.
    #[must_use]
    pub fn model(&self, type_name: &str, alias: &str) -> Option<Arc<dyn Model>> {
        self.models
            .get(&ComponentKey::new(type_name, alias))
            .cloned()
    }

    /// Looks up a built [`Memory`] backend by `type`/`alias`.
    #[must_use]
    pub fn memory(&self, type_name: &str, alias: &str) -> Option<Arc<dyn Memory>> {
        self.memories
            .get(&ComponentKey::new(type_name, alias))
            .cloned()
    }

    /// Iterates every built channel as `(key, instance)`.
    pub fn channels(&self) -> impl Iterator<Item = (&ComponentKey, &Arc<dyn Channel>)> {
        self.channels.iter()
    }

    /// Iterates every built model provider as `(key, instance)`. The
    /// Model_Router consumes this to select among providers (R2.3).
    pub fn models(&self) -> impl Iterator<Item = (&ComponentKey, &Arc<dyn Model>)> {
        self.models.iter()
    }

    /// Iterates every built memory backend as `(key, instance)`.
    pub fn memories(&self) -> impl Iterator<Item = (&ComponentKey, &Arc<dyn Memory>)> {
        self.memories.iter()
    }

    /// Returns the number of built channels.
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Returns the number of built model providers.
    #[must_use]
    pub fn model_count(&self) -> usize {
        self.models.len()
    }

    /// Returns the number of built memory backends.
    #[must_use]
    pub fn memory_count(&self) -> usize {
        self.memories.len()
    }

    /// Returns the failures collected by the most recent [`load`](Self::load).
    #[must_use]
    pub fn failures(&self) -> &[LoadFailure] {
        &self.failures
    }

    /// Returns `true` if the most recent [`load`](Self::load) skipped any
    /// component.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{ComponentKey, ComponentKind, LoadError, PluginRegistry};
    use crate::{Config, SecretResolver};
    use async_trait::async_trait;
    use std::sync::Arc;

    use cyrene_core::{
        Channel, ChannelError, ChannelHealth, ChannelId, Fact, InboundMessage, Memory, MemoryError,
        MemoryHit, MemoryQuery, Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse,
        Money, NodeId, OutboundMessage, Relation, Tier,
    };

    /// A config declaring two providers, two channels, and one memory backend.
    const SAMPLE: &str = r#"
[providers.openai.coding]
model = "gpt-4o"
tier  = "Premium"

[providers.ollama.local]
model = "llama3.1"
tier  = "Local"

[channels.telegram.personal]
token_env = "CYRENE_TEST_TG_TOKEN"

[channels.cli.default]

[memory.sqlite.default]
path = "/tmp/cyrene-test.db"
"#;

    // ---- Minimal in-test implementations of the three plugin traits. ----
    // Trait methods are trivial so the registry can be exercised without a
    // full async runtime; the registry never awaits them, it only builds and
    // stores the `Arc<dyn …>`.

    struct FakeModel {
        descriptor: ModelDescriptor,
    }

    #[async_trait]
    impl Model for FakeModel {
        fn descriptor(&self) -> ModelDescriptor {
            self.descriptor.clone()
        }
        async fn complete(&self, _req: ModelRequest) -> Result<ModelResponse, ModelError> {
            Ok(ModelResponse::new(
                "ok",
                cyrene_core::TokenUsage::new(0, 0),
                cyrene_core::FinishReason::Stop,
            ))
        }
        async fn embed(&self, _text: &str) -> Result<Vec<f32>, ModelError> {
            Ok(Vec::new())
        }
    }

    struct FakeChannel {
        id: ChannelId,
    }

    #[async_trait]
    impl Channel for FakeChannel {
        fn id(&self) -> ChannelId {
            self.id.clone()
        }
        async fn poll(&self) -> Result<Option<InboundMessage>, ChannelError> {
            Ok(None)
        }
        async fn send(&self, _msg: OutboundMessage) -> Result<(), ChannelError> {
            Ok(())
        }
        async fn health(&self) -> ChannelHealth {
            ChannelHealth::Healthy
        }
    }

    struct FakeMemory;

    #[async_trait]
    impl Memory for FakeMemory {
        async fn upsert_fact(&self, _fact: Fact) -> Result<NodeId, MemoryError> {
            Ok(NodeId::new())
        }
        async fn query(&self, _q: MemoryQuery) -> Result<Vec<MemoryHit>, MemoryError> {
            Ok(Vec::new())
        }
        async fn link(
            &self,
            _from: NodeId,
            _rel: Relation,
            _to: NodeId,
        ) -> Result<(), MemoryError> {
            Ok(())
        }
    }

    /// A registry with a working factory for every `type` used in `SAMPLE`.
    fn registry_with_all_factories() -> PluginRegistry {
        let mut reg = PluginRegistry::new();

        let model_factory = |ctx: super::BuildContext<'_, crate::ProviderEntry>| {
            Ok(Arc::new(FakeModel {
                descriptor: ModelDescriptor {
                    alias: ctx.alias.to_owned(),
                    tier: ctx.entry.tier.unwrap_or(Tier::Local),
                    input_price: Money::new("USD", 0),
                    output_price: Money::new("USD", 0),
                },
            }) as Arc<dyn Model>)
        };
        reg.register_model("openai", model_factory);
        reg.register_model("ollama", model_factory);

        let channel_factory = |ctx: super::BuildContext<'_, crate::ChannelEntry>| {
            Ok(Arc::new(FakeChannel {
                id: ChannelId::new(ctx.alias),
            }) as Arc<dyn Channel>)
        };
        reg.register_channel("telegram", channel_factory);
        reg.register_channel("cli", channel_factory);

        reg.register_memory("sqlite", |_ctx| Ok(Arc::new(FakeMemory) as Arc<dyn Memory>));

        reg
    }

    #[test]
    fn good_config_builds_all_components() {
        let cfg = Config::parse(SAMPLE, "t.toml").unwrap();
        let secrets = SecretResolver::from_env();
        let mut reg = registry_with_all_factories();

        let failures = reg.load(&cfg, &secrets);
        assert!(failures.is_empty(), "no component should fail to load");

        // Every declared component is present and looked up by its alias key.
        assert!(reg.model("openai", "coding").is_some());
        assert!(reg.model("ollama", "local").is_some());
        assert!(reg.channel("telegram", "personal").is_some());
        assert!(reg.channel("cli", "default").is_some());
        assert!(reg.memory("sqlite", "default").is_some());

        assert_eq!(reg.model_count(), 2);
        assert_eq!(reg.channel_count(), 2);
        assert_eq!(reg.memory_count(), 1);

        // The factory received the alias and threaded it into the instance.
        assert_eq!(
            reg.model("openai", "coding").unwrap().descriptor().alias,
            "coding"
        );
        assert_eq!(
            reg.model("openai", "coding").unwrap().descriptor().tier,
            Tier::Premium
        );
        assert_eq!(
            reg.channel("cli", "default").unwrap().id(),
            ChannelId::new("default")
        );
    }

    #[test]
    fn one_failing_component_is_isolated_and_reported() {
        let cfg = Config::parse(SAMPLE, "t.toml").unwrap();
        let secrets = SecretResolver::from_env();
        let mut reg = registry_with_all_factories();

        // Make just the openai provider's factory fail at init time.
        reg.register_model("openai", |_ctx| Err("simulated init failure".into()));

        let failures = reg.load(&cfg, &secrets);
        assert_eq!(failures.len(), 1, "exactly one component should fail");
        assert_eq!(failures[0].kind, ComponentKind::Model);
        assert_eq!(failures[0].key, ComponentKey::new("openai", "coding"));
        assert!(matches!(failures[0].error, LoadError::Init(_)));

        // The failing component is absent...
        assert!(reg.model("openai", "coding").is_none());
        // ...but every other component still loaded (R2.4: continue with the rest).
        assert!(reg.model("ollama", "local").is_some());
        assert!(reg.channel("telegram", "personal").is_some());
        assert!(reg.channel("cli", "default").is_some());
        assert!(reg.memory("sqlite", "default").is_some());
        assert!(reg.has_failures());
    }

    #[test]
    fn missing_factory_is_reported_not_panicked() {
        // The config references provider type `mystery`, which has no factory.
        let toml = "[providers.mystery.x]\nmodel = \"m\"\n[channels.cli.default]\n";
        let cfg = Config::parse(toml, "t.toml").unwrap();
        let secrets = SecretResolver::from_env();

        let mut reg = PluginRegistry::new();
        reg.register_channel("cli", |ctx| {
            Ok(Arc::new(FakeChannel {
                id: ChannelId::new(ctx.alias),
            }) as Arc<dyn Channel>)
        });

        // No panic; the unregistered type is collected as a NoFactory failure.
        let failures = reg.load(&cfg, &secrets);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].kind, ComponentKind::Model);
        assert_eq!(failures[0].key, ComponentKey::new("mystery", "x"));
        assert!(matches!(failures[0].error, LoadError::NoFactory(_)));

        // The component that did have a factory still loaded.
        assert!(reg.channel("cli", "default").is_some());
        assert_eq!(reg.model_count(), 0);
    }

    #[test]
    fn disabled_components_are_skipped_without_failure() {
        // openai is disabled and has no factory: it must be skipped silently,
        // NOT reported as a missing-factory failure.
        let toml = "[providers.openai.coding]\nenabled = false\n[channels.cli.default]\n";
        let cfg = Config::parse(toml, "t.toml").unwrap();
        let secrets = SecretResolver::from_env();

        let mut reg = PluginRegistry::new();
        reg.register_channel("cli", |ctx| {
            Ok(Arc::new(FakeChannel {
                id: ChannelId::new(ctx.alias),
            }) as Arc<dyn Channel>)
        });

        let failures = reg.load(&cfg, &secrets);
        assert!(
            failures.is_empty(),
            "a disabled component must not be a load failure"
        );
        assert_eq!(reg.model_count(), 0);
        assert!(reg.channel("cli", "default").is_some());
    }

    #[test]
    fn missing_providers_section_builds_zero_models_but_loads_channels() {
        // A config with a `channels` section but no `[providers.*]` section at
        // all. The registry has nothing to build for models, so the model
        // lookup table stays empty while the declared channel still loads.
        // (The config layer rejects this same shape via `Config::validate`;
        // that is covered by `config::tests::validate_requires_provider_and_channel`
        // and the `load_from_path` integration test. Here we assert the
        // registry's behaviour when handed such a config directly.)
        let toml = "[channels.cli.default]\n";
        let cfg = Config::parse(toml, "t.toml").unwrap();
        let secrets = SecretResolver::from_env();

        let mut reg = PluginRegistry::new();
        reg.register_channel("cli", |ctx| {
            Ok(Arc::new(FakeChannel {
                id: ChannelId::new(ctx.alias),
            }) as Arc<dyn Channel>)
        });
        // A model factory IS registered, so a zero model count can only be
        // because the providers section is absent — not a missing factory
        // (which would otherwise surface as a `NoFactory` load failure).
        reg.register_model("openai", |ctx| {
            Ok(Arc::new(FakeModel {
                descriptor: ModelDescriptor {
                    alias: ctx.alias.to_owned(),
                    tier: ctx.entry.tier.unwrap_or(Tier::Local),
                    input_price: Money::new("USD", 0),
                    output_price: Money::new("USD", 0),
                },
            }) as Arc<dyn Model>)
        });

        let failures = reg.load(&cfg, &secrets);
        // A missing section is not a load failure: there is simply nothing to
        // build for that kind.
        assert!(
            failures.is_empty(),
            "a missing section must not be a load failure"
        );
        // Zero models are built because the providers section is absent...
        assert_eq!(reg.model_count(), 0);
        assert!(reg.models().next().is_none());
        // ...while the declared channel still loads.
        assert_eq!(reg.channel_count(), 1);
        assert!(reg.channel("cli", "default").is_some());
    }

    #[test]
    fn load_is_idempotent_and_clears_previous_state() {
        let cfg = Config::parse(SAMPLE, "t.toml").unwrap();
        let secrets = SecretResolver::from_env();
        let mut reg = registry_with_all_factories();

        reg.load(&cfg, &secrets);
        assert_eq!(reg.model_count(), 2);

        // Reloading with the same factories does not duplicate instances.
        reg.load(&cfg, &secrets);
        assert_eq!(reg.model_count(), 2);
        assert_eq!(reg.channel_count(), 2);
        assert!(!reg.has_failures());
    }

    #[test]
    fn load_failure_display_is_human_readable() {
        let toml = "[providers.mystery.x]\n[channels.cli.default]\n";
        let cfg = Config::parse(toml, "t.toml").unwrap();
        let secrets = SecretResolver::from_env();

        let mut reg = PluginRegistry::new();
        reg.register_channel("cli", |ctx| {
            Ok(Arc::new(FakeChannel {
                id: ChannelId::new(ctx.alias),
            }) as Arc<dyn Channel>)
        });

        let failures = reg.load(&cfg, &secrets);
        let rendered = failures[0].to_string();
        assert!(rendered.contains("model `mystery.x`"));
        assert!(rendered.contains("no factory registered"));
    }
}
