//! `cyrene-models`: Model_Provider integrations for Cyrene.

pub mod budget_guard;
pub mod providers;
pub mod router;

use cyrene_config::{BoxError, ProviderEntry, SecretResolver};
use cyrene_core::Model;
use providers::*;
use std::sync::Arc;

pub use budget_guard::{BudgetAction, BudgetGuard};
pub use router::{EscalationEvent, ModelRouter, RouterError};

/// Creates a provider instance by its config `type_name`.
pub fn create_provider(
    type_name: &str,
    alias: &str,
    entry: &ProviderEntry,
    secrets: &SecretResolver,
) -> Result<Arc<dyn Model>, BoxError> {
    match type_name {
        "openai" => Ok(Arc::new(OpenAiProvider::new(alias, entry, secrets)?)),
        "anthropic" => Ok(Arc::new(AnthropicProvider::new(alias, entry, secrets)?)),
        "openrouter" => Ok(Arc::new(OpenRouterProvider::new(alias, entry, secrets)?)),
        "gemini" => Ok(Arc::new(GeminiProvider::new(alias, entry, secrets)?)),
        "ollama" => Ok(Arc::new(OllamaProvider::new(alias, entry)?)),
        "openai_compat" => Ok(Arc::new(OpenAiCompatProvider::new(alias, entry, secrets)?)),
        // Every hosted OpenAI-compatible provider (groq, xai, mistral,
        // deepseek, cohere, qwen, perplexity, moonshot, minimax, together,
        // xiaomi, opencode, …) is served from the preset table.
        other => match presets::find(other) {
            Some(preset) => Ok(Arc::new(PresetProvider::new(
                preset, alias, entry, secrets,
            )?)),
            None => Err(format!("unknown provider type: `{type_name}`").into()),
        },
    }
}

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-models"
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrene_config::SecretResolver;
    use cyrene_core::Tier;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }

    #[test]
    fn create_provider_unknown_type_errors() {
        let entry = ProviderEntry::default();
        let secrets = SecretResolver::from_env();
        assert!(create_provider("nonexistent", "x", &entry, &secrets).is_err());
    }

    #[test]
    fn ollama_succeeds_without_api_key() {
        let entry = ProviderEntry {
            model: Some("llama3.1".to_owned()),
            tier: Some(Tier::Local),
            ..Default::default()
        };
        let secrets = SecretResolver::from_env();
        let p = create_provider("ollama", "local", &entry, &secrets).unwrap();
        assert_eq!(p.descriptor().alias, "local");
        assert_eq!(p.descriptor().tier, Tier::Local);
    }

    #[test]
    fn ollama_has_zero_pricing() {
        let entry = ProviderEntry {
            model: Some("llama3.1".to_owned()),
            tier: Some(Tier::Local),
            ..Default::default()
        };
        let secrets = SecretResolver::from_env();
        let p = create_provider("ollama", "local", &entry, &secrets).unwrap();
        assert!(p.descriptor().input_price.is_zero());
        assert!(p.descriptor().output_price.is_zero());
    }

    #[test]
    fn openai_missing_key_errors() {
        let key = "CYRENE_TEST_OPENAI_MISSING_KEY_XYZ";
        std::env::remove_var(key);
        let entry = ProviderEntry {
            model: Some("gpt-4o".to_owned()),
            api_key_env: Some(key.to_owned()),
            ..Default::default()
        };
        let secrets = SecretResolver::from_env();
        assert!(create_provider("openai", "test", &entry, &secrets).is_err());
    }

    fn with_fake_key(key_name: &str, f: impl FnOnce()) {
        std::env::set_var(key_name, "sk-fake-test-key");
        f();
        std::env::remove_var(key_name);
    }

    #[test]
    fn openai_with_key_succeeds() {
        let key = "CYRENE_TEST_OPENAI_KEY_8_1A";
        with_fake_key(key, || {
            let entry = ProviderEntry {
                model: Some("gpt-4o".to_owned()),
                tier: Some(Tier::Premium),
                api_key_env: Some(key.to_owned()),
                ..Default::default()
            };
            let secrets = SecretResolver::from_env();
            let p = create_provider("openai", "coding", &entry, &secrets).unwrap();
            assert_eq!(p.descriptor().alias, "coding");
            assert_eq!(p.descriptor().tier, Tier::Premium);
        });
    }

    #[test]
    fn anthropic_with_key_succeeds() {
        let key = "CYRENE_TEST_ANTHROPIC_KEY_8_1B";
        with_fake_key(key, || {
            let entry = ProviderEntry {
                model: Some("claude-3-5-sonnet".to_owned()),
                tier: Some(Tier::Premium),
                api_key_env: Some(key.to_owned()),
                ..Default::default()
            };
            let secrets = SecretResolver::from_env();
            let p = create_provider("anthropic", "claude", &entry, &secrets).unwrap();
            assert_eq!(p.descriptor().alias, "claude");
        });
    }

    #[test]
    fn openrouter_with_key_succeeds() {
        let key = "CYRENE_TEST_OPENROUTER_KEY_8_1C";
        with_fake_key(key, || {
            let entry = ProviderEntry {
                model: Some("anthropic/claude-3.5-sonnet".to_owned()),
                tier: Some(Tier::Premium),
                api_key_env: Some(key.to_owned()),
                ..Default::default()
            };
            let secrets = SecretResolver::from_env();
            let p = create_provider("openrouter", "router", &entry, &secrets).unwrap();
            assert_eq!(p.descriptor().alias, "router");
        });
    }

    #[test]
    fn gemini_with_key_succeeds() {
        let key = "CYRENE_TEST_GEMINI_KEY_8_1D";
        with_fake_key(key, || {
            let entry = ProviderEntry {
                model: Some("gemini-1.5-pro".to_owned()),
                tier: Some(Tier::Premium),
                api_key_env: Some(key.to_owned()),
                ..Default::default()
            };
            let secrets = SecretResolver::from_env();
            let p = create_provider("gemini", "google", &entry, &secrets).unwrap();
            assert_eq!(p.descriptor().alias, "google");
        });
    }

    #[test]
    fn openai_compat_with_key_succeeds() {
        let key = "CYRENE_TEST_COMPAT_KEY_8_1E";
        with_fake_key(key, || {
            let entry = ProviderEntry {
                model: Some("deepseek-chat".to_owned()),
                tier: Some(Tier::Premium),
                base_url: Some("https://api.deepseek.com/v1".to_owned()),
                api_key_env: Some(key.to_owned()),
                ..Default::default()
            };
            let secrets = SecretResolver::from_env();
            let p = create_provider("openai_compat", "deepseek", &entry, &secrets).unwrap();
            assert_eq!(p.descriptor().alias, "deepseek");
        });
    }

    #[test]
    fn preset_providers_with_key_succeed() {
        let key = "CYRENE_TEST_PRESET_KEY_8_1F";
        with_fake_key(key, || {
            let secrets = SecretResolver::from_env();
            for preset in PRESETS {
                let entry = ProviderEntry {
                    tier: Some(Tier::Premium),
                    api_key_env: Some(key.to_owned()),
                    ..Default::default()
                };
                let p = create_provider(preset.type_name, preset.type_name, &entry, &secrets)
                    .unwrap_or_else(|e| panic!("preset `{}` failed: {e}", preset.type_name));
                assert_eq!(p.descriptor().alias, preset.type_name);
                assert_eq!(p.descriptor().tier, Tier::Premium);
            }
        });
    }

    #[test]
    fn preset_provider_requires_api_key() {
        let key = "CYRENE_TEST_PRESET_MISSING_KEY_XYZ";
        std::env::remove_var(key);
        let entry = ProviderEntry {
            api_key_env: Some(key.to_owned()),
            ..Default::default()
        };
        let secrets = SecretResolver::from_env();
        assert!(create_provider("groq", "g", &entry, &secrets).is_err());
    }

    #[test]
    fn preset_type_names_are_unique() {
        let mut names: Vec<&str> = PRESETS.iter().map(|p| p.type_name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "preset type_name values must be unique");
    }
}
