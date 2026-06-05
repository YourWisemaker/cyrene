//! The single-file TOML configuration model and loader (R2.5).
//!
//! The whole agent is configured by **one** human-editable TOML file (default
//! `~/.cyrene/config.toml`). Components are declared in a `[type.alias]` shape
//! so a user can register several instances of the same component type under
//! distinct aliases — for example:
//!
//! ```toml
//! [providers.openai.coding]      # type = "openai", alias = "coding"
//! model = "gpt-4o"
//! tier  = "Premium"
//! api_key_env = "OPENAI_API_KEY" # references a secret by env-var NAME only
//!
//! [providers.ollama.local]       # type = "ollama", alias = "local"
//! model = "llama3.1"
//! tier  = "Local"
//!
//! [channels.telegram.personal]   # type = "telegram", alias = "personal"
//! token_env = "TELEGRAM_BOT_TOKEN"
//!
//! [memory.sqlite.default]        # type = "sqlite", alias = "default"
//! path = "~/.cyrene/cyrene.db"
//! ```
//!
//! Crucially, **no secret values live in this file** — entries reference the
//! *name* of the environment variable holding the secret (e.g. `api_key_env`),
//! and the actual value is read at runtime by the
//! [`SecretResolver`](crate::SecretResolver) from the environment / `.env`
//! (R22, secret hygiene). The TOML is safe to commit or share.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cyrene_core::Tier;
use serde::{Deserialize, Serialize};

use crate::autonomy::AutonomyConfig;
use crate::error::ConfigError;
use crate::execution::ExecutionConfig;

/// A map of `alias -> entry` for one component `type`.
pub type AliasMap<T> = BTreeMap<String, T>;

/// A map of `type -> { alias -> entry }`, mirroring the `[type.alias]` TOML
/// nesting (e.g. `providers.openai.coding`).
pub type TypeAliasMap<T> = BTreeMap<String, AliasMap<T>>;

/// The fully-qualified coordinates of a declared component: its `type` and
/// `alias` (e.g. type `"openai"`, alias `"coding"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentRef<'a, T> {
    /// The component type (the first TOML key, e.g. `"openai"`).
    pub type_name: &'a str,
    /// The component alias (the second TOML key, e.g. `"coding"`).
    pub alias: &'a str,
    /// The deserialized entry settings.
    pub entry: &'a T,
}

/// A configured Model_Provider entry under `[providers.<type>.<alias>]`.
///
/// Holds only non-secret settings. Credentials are referenced by environment
/// variable name via [`ProviderEntry::api_key_env`] and resolved at runtime;
/// the secret value never appears in the config file (R12 note, R22).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderEntry {
    /// The provider's model name (e.g. `"gpt-4o"`, `"claude-3-5-sonnet"`).
    pub model: Option<String>,
    /// Base URL override, used by the generic OpenAI-compatible provider for
    /// DeepSeek / xAI / Groq / self-hosted endpoints.
    pub base_url: Option<String>,
    /// The cost/capability tier the Model_Router reasons about (R12).
    pub tier: Option<Tier>,
    /// Name of the environment variable holding this provider's API key.
    /// The value is resolved from env/`.env`, never stored here.
    pub api_key_env: Option<String>,
    /// Whether this entry is enabled. Defaults to `true`.
    pub enabled: bool,
}

impl Default for ProviderEntry {
    fn default() -> Self {
        Self {
            model: None,
            base_url: None,
            tier: None,
            api_key_env: None,
            enabled: true,
        }
    }
}

/// A configured Channel entry under `[channels.<type>.<alias>]`.
///
/// Tokens/credentials are referenced by environment-variable name only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChannelEntry {
    /// Name of the environment variable holding this channel's auth token
    /// (e.g. a Telegram bot token, Slack token). Resolved from env/`.env`.
    pub token_env: Option<String>,
    /// Allowlist of user identifiers permitted to message inbound (R22.5,
    /// R7.3 DM pairing/allowlist). Empty means no inbound users are permitted
    /// until explicitly added.
    pub allowlist: Vec<String>,
    /// Whether this entry is enabled. Defaults to `true`.
    pub enabled: bool,
}

impl Default for ChannelEntry {
    fn default() -> Self {
        Self {
            token_env: None,
            allowlist: Vec::new(),
            enabled: true,
        }
    }
}

/// A configured Memory backend entry under `[memory.<type>.<alias>]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryEntry {
    /// Filesystem path for file-backed memory (e.g. the SQLite graph DB).
    pub path: Option<String>,
    /// Connection URL for a networked memory backend, referencing a secret by
    /// env-var name when credentials are required.
    pub url_env: Option<String>,
    /// Whether this entry is enabled. Defaults to `true`.
    pub enabled: bool,
}

impl Default for MemoryEntry {
    fn default() -> Self {
        Self {
            path: None,
            url_env: None,
            enabled: true,
        }
    }
}

/// The complete, deserialized contents of the single Cyrene config file.
///
/// All sections default to empty/secure so a partial file still loads; the
/// loader separately enforces that the minimum required sections are present
/// (see [`Config::load_from_path`]).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Model_Provider declarations keyed `providers.<type>.<alias>`.
    pub providers: TypeAliasMap<ProviderEntry>,
    /// Channel declarations keyed `channels.<type>.<alias>`.
    pub channels: TypeAliasMap<ChannelEntry>,
    /// Memory backend declarations keyed `memory.<type>.<alias>`.
    pub memory: TypeAliasMap<MemoryEntry>,
    /// Autonomy / security policy. Secure-by-default when omitted (R22).
    pub autonomy: AutonomyConfig,
    /// Remote execution backend. Defaults to local sandboxed execution; a
    /// remote backend (SSH / container host) relocates where Steps run while
    /// preserving the autonomy/sandbox/approval constraints (R33.5).
    pub execution: ExecutionConfig,
}

impl Config {
    /// Returns the default config file path: `~/.cyrene/config.toml`.
    ///
    /// # Errors
    /// Returns [`ConfigError::NoHomeDir`] if the home directory cannot be
    /// determined.
    pub fn default_path() -> Result<PathBuf, ConfigError> {
        let home = dirs::home_dir().ok_or(ConfigError::NoHomeDir)?;
        Ok(home.join(".cyrene").join("config.toml"))
    }

    /// Loads and validates the config from the default path
    /// (`~/.cyrene/config.toml`).
    ///
    /// # Errors
    /// See [`Config::load_from_path`].
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from_path(Self::default_path()?)
    }

    /// Loads and validates the config from an explicit file path.
    ///
    /// Validation requires at least one Model_Provider and one Channel so the
    /// runtime can both think and communicate (R2.5). Autonomy/memory may be
    /// omitted and fall back to secure/empty defaults.
    ///
    /// # Errors
    /// - [`ConfigError::Io`] if the file cannot be read.
    /// - [`ConfigError::Parse`] if the file is not valid TOML for this schema.
    /// - [`ConfigError::MissingSection`] if a required section is absent.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let config = Self::parse(&raw, path)?;
        config.validate()?;
        Ok(config)
    }

    /// Parses config from a TOML string without touching the filesystem.
    ///
    /// `path` is used only for error reporting. This does not run validation;
    /// use [`Config::validate`] (or [`Config::load_from_path`]) for that.
    ///
    /// # Errors
    /// Returns [`ConfigError::Parse`] if the string is not valid TOML for the
    /// config schema.
    pub fn parse(raw: &str, path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        toml::from_str(raw).map_err(|source| ConfigError::Parse {
            path: path.as_ref().to_path_buf(),
            source,
        })
    }

    /// Validates that the required sections are present.
    ///
    /// # Errors
    /// Returns [`ConfigError::MissingSection`] naming the first missing
    /// required section (`providers`, then `channels`), or
    /// [`ConfigError::InvalidExecution`] if a selected remote execution backend
    /// is missing required settings (R33.5).
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.providers.is_empty() {
            return Err(ConfigError::MissingSection("providers"));
        }
        if self.channels.is_empty() {
            return Err(ConfigError::MissingSection("channels"));
        }
        self.execution
            .validate()
            .map_err(ConfigError::InvalidExecution)?;
        Ok(())
    }

    /// Iterates every declared provider as a flattened [`ComponentRef`].
    pub fn providers(&self) -> impl Iterator<Item = ComponentRef<'_, ProviderEntry>> {
        flatten(&self.providers)
    }

    /// Iterates every declared channel as a flattened [`ComponentRef`].
    pub fn channels(&self) -> impl Iterator<Item = ComponentRef<'_, ChannelEntry>> {
        flatten(&self.channels)
    }

    /// Iterates every declared memory backend as a flattened [`ComponentRef`].
    pub fn memory(&self) -> impl Iterator<Item = ComponentRef<'_, MemoryEntry>> {
        flatten(&self.memory)
    }

    /// Looks up a single provider by `type` and `alias`.
    #[must_use]
    pub fn provider(&self, type_name: &str, alias: &str) -> Option<&ProviderEntry> {
        self.providers.get(type_name).and_then(|m| m.get(alias))
    }

    /// Looks up a single channel by `type` and `alias`.
    #[must_use]
    pub fn channel(&self, type_name: &str, alias: &str) -> Option<&ChannelEntry> {
        self.channels.get(type_name).and_then(|m| m.get(alias))
    }

    /// Collects the names of every secret environment variable referenced by
    /// the config (provider `api_key_env`, channel `token_env`, memory
    /// `url_env`). Useful for a `doctor`-style check that every referenced
    /// secret is actually present in the environment.
    #[must_use]
    pub fn referenced_secret_envs(&self) -> Vec<String> {
        let mut names = Vec::new();
        for p in self.providers() {
            if let Some(env) = &p.entry.api_key_env {
                names.push(env.clone());
            }
        }
        for c in self.channels() {
            if let Some(env) = &c.entry.token_env {
                names.push(env.clone());
            }
        }
        for m in self.memory() {
            if let Some(env) = &m.entry.url_env {
                names.push(env.clone());
            }
        }
        names.extend(self.execution.referenced_secret_envs());
        names.sort();
        names.dedup();
        names
    }
}

/// Flattens a `type -> { alias -> entry }` map into [`ComponentRef`]s.
fn flatten<T>(map: &TypeAliasMap<T>) -> impl Iterator<Item = ComponentRef<'_, T>> {
    map.iter().flat_map(|(type_name, aliases)| {
        aliases.iter().map(move |(alias, entry)| ComponentRef {
            type_name,
            alias,
            entry,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::Config;
    use crate::autonomy::AutonomyAction;
    use cyrene_core::{Risk, Tier};

    const SAMPLE: &str = r#"
[providers.openai.coding]
model = "gpt-4o"
tier  = "Premium"
api_key_env = "OPENAI_API_KEY"

[providers.ollama.local]
model = "llama3.1"
tier  = "Local"

[channels.telegram.personal]
token_env = "TELEGRAM_BOT_TOKEN"
allowlist = ["123456"]

[channels.cli.default]

[memory.sqlite.default]
path = "~/.cyrene/cyrene.db"
"#;

    #[test]
    fn parses_type_alias_sections() {
        let cfg = Config::parse(SAMPLE, "test.toml").unwrap();

        let openai = cfg.provider("openai", "coding").unwrap();
        assert_eq!(openai.model.as_deref(), Some("gpt-4o"));
        assert_eq!(openai.tier, Some(Tier::Premium));
        assert_eq!(openai.api_key_env.as_deref(), Some("OPENAI_API_KEY"));
        assert!(openai.enabled);

        let ollama = cfg.provider("ollama", "local").unwrap();
        assert_eq!(ollama.tier, Some(Tier::Local));

        let tg = cfg.channel("telegram", "personal").unwrap();
        assert_eq!(tg.token_env.as_deref(), Some("TELEGRAM_BOT_TOKEN"));
        assert_eq!(tg.allowlist, vec!["123456".to_owned()]);

        let mem = cfg.memory.get("sqlite").unwrap().get("default").unwrap();
        assert_eq!(mem.path.as_deref(), Some("~/.cyrene/cyrene.db"));
    }

    #[test]
    fn flattened_iterators_expose_type_and_alias() {
        let cfg = Config::parse(SAMPLE, "test.toml").unwrap();

        let mut providers: Vec<_> = cfg
            .providers()
            .map(|p| (p.type_name.to_owned(), p.alias.to_owned()))
            .collect();
        providers.sort();
        assert_eq!(
            providers,
            vec![
                ("ollama".to_owned(), "local".to_owned()),
                ("openai".to_owned(), "coding".to_owned()),
            ]
        );

        let channels: Vec<_> = cfg
            .channels()
            .map(|c| (c.type_name.to_owned(), c.alias.to_owned()))
            .collect();
        assert!(channels.contains(&("cli".to_owned(), "default".to_owned())));
        assert!(channels.contains(&("telegram".to_owned(), "personal".to_owned())));
    }

    #[test]
    fn autonomy_defaults_apply_when_section_omitted() {
        let cfg = Config::parse(SAMPLE, "test.toml").unwrap();
        assert_eq!(
            cfg.autonomy.action_for(Risk::Medium),
            AutonomyAction::Approval
        );
        assert_eq!(cfg.autonomy.action_for(Risk::High), AutonomyAction::Blocked);
        assert!(cfg.autonomy.require_gateway_auth);
    }

    #[test]
    fn explicit_autonomy_section_overrides_defaults() {
        let toml =
            format!("{SAMPLE}\n[autonomy]\nmedium = \"auto\"\ncommand_allowlist = [\"git\"]\n");
        let cfg = Config::parse(&toml, "test.toml").unwrap();
        assert_eq!(cfg.autonomy.action_for(Risk::Medium), AutonomyAction::Auto);
        // Unspecified fields still hold the secure default.
        assert_eq!(cfg.autonomy.action_for(Risk::High), AutonomyAction::Blocked);
        assert!(cfg.autonomy.is_command_allowed("git status"));
    }

    #[test]
    fn validate_requires_provider_and_channel() {
        // No providers.
        let only_channel = "[channels.cli.default]\n";
        let cfg = Config::parse(only_channel, "t.toml").unwrap();
        assert!(matches!(
            cfg.validate(),
            Err(crate::ConfigError::MissingSection("providers"))
        ));

        // Providers but no channels.
        let only_provider = "[providers.ollama.local]\nmodel = \"llama3.1\"\n";
        let cfg = Config::parse(only_provider, "t.toml").unwrap();
        assert!(matches!(
            cfg.validate(),
            Err(crate::ConfigError::MissingSection("channels"))
        ));
    }

    #[test]
    fn malformed_toml_is_a_parse_error() {
        let bad = "[providers.openai.coding]\nmodel = ";
        assert!(matches!(
            Config::parse(bad, "bad.toml"),
            Err(crate::ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        // `nonsense` is not a known top-level section; `deny_unknown_fields`
        // rejects it rather than silently ignoring a likely typo.
        let toml = "nonsense = true\n[providers.ollama.local]\n[channels.cli.default]\n";
        assert!(Config::parse(toml, "t.toml").is_err());
    }

    #[test]
    fn unknown_provider_field_is_rejected() {
        let bad = "[providers.openai.coding]\napi_key = \"sk-should-not-be-here\"\n";
        // A literal `api_key` (a secret) must NOT be accepted; only `api_key_env`.
        assert!(Config::parse(bad, "t.toml").is_err());
    }

    #[test]
    fn referenced_secret_envs_are_collected_and_deduped() {
        let cfg = Config::parse(SAMPLE, "test.toml").unwrap();
        let secrets = cfg.referenced_secret_envs();
        assert_eq!(secrets, vec!["OPENAI_API_KEY", "TELEGRAM_BOT_TOKEN"]);
    }

    #[test]
    fn full_round_trip_through_toml() {
        let cfg = Config::parse(SAMPLE, "test.toml").unwrap();
        let serialized = toml::to_string(&cfg).unwrap();
        let back = Config::parse(&serialized, "rt.toml").unwrap();
        assert_eq!(cfg, back);
    }
}
