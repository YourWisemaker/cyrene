//! Table-driven presets for hosted, OpenAI-compatible providers.
//!
//! A large and growing number of providers (Groq, xAI, Mistral, DeepSeek,
//! Cohere, Alibaba Qwen, Perplexity, Moonshot/Kimi, MiniMax, Together, Xiaomi
//! MiMo, OpenCode Zen, …) all speak the OpenAI `chat/completions` wire format
//! and differ only by base URL, a sensible default model, and list pricing.
//!
//! Rather than a near-identical source file per provider, each is described by
//! a [`Preset`] row and served by the single generic [`PresetProvider`]. The
//! base URL, model, and tier remain overridable through the config entry, so a
//! preset is just a convenient set of defaults (e.g. regional endpoints or
//! self-hosted gateways can override `base_url`).

use async_trait::async_trait;
use cyrene_config::{BoxError, ProviderEntry, SecretResolver};
use cyrene_core::{Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Money, Tier};
use reqwest::Client;

use super::openai::{openai_chat_complete, resolve_api_key};

/// Defaults for one hosted OpenAI-compatible provider.
pub struct Preset {
    /// Config `type` used in `[providers.<type>.<alias>]`.
    pub type_name: &'static str,
    /// Fixed API base URL (no trailing slash, no `/chat/completions` suffix).
    pub base_url: &'static str,
    /// Model id used when the config entry omits `model`.
    pub default_model: &'static str,
    /// Input list price in USD cents per 1M tokens (`0` = unknown / gateway).
    pub input_cents_per_mtok: i64,
    /// Output list price in USD cents per 1M tokens (`0` = unknown / gateway).
    pub output_cents_per_mtok: i64,
}

/// Every built-in OpenAI-compatible provider preset.
///
/// Prices are approximate published list prices and are overshadowed by the
/// provider's real billing; gateways that route to many models report `0`.
pub const PRESETS: &[Preset] = &[
    Preset {
        type_name: "groq",
        base_url: "https://api.groq.com/openai/v1",
        default_model: "llama-3.3-70b-versatile",
        input_cents_per_mtok: 59,
        output_cents_per_mtok: 79,
    },
    Preset {
        type_name: "xai",
        base_url: "https://api.x.ai/v1",
        default_model: "grok-2-latest",
        input_cents_per_mtok: 200,
        output_cents_per_mtok: 1000,
    },
    Preset {
        type_name: "mistral",
        base_url: "https://api.mistral.ai/v1",
        default_model: "mistral-large-latest",
        input_cents_per_mtok: 200,
        output_cents_per_mtok: 600,
    },
    Preset {
        type_name: "deepseek",
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-chat",
        input_cents_per_mtok: 27,
        output_cents_per_mtok: 110,
    },
    Preset {
        type_name: "cohere",
        base_url: "https://api.cohere.ai/compatibility/v1",
        default_model: "command-a-03-2025",
        input_cents_per_mtok: 250,
        output_cents_per_mtok: 1000,
    },
    Preset {
        type_name: "qwen",
        base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen-max",
        input_cents_per_mtok: 160,
        output_cents_per_mtok: 640,
    },
    Preset {
        type_name: "perplexity",
        base_url: "https://api.perplexity.ai",
        default_model: "sonar",
        input_cents_per_mtok: 100,
        output_cents_per_mtok: 100,
    },
    Preset {
        type_name: "moonshot",
        base_url: "https://api.moonshot.ai/v1",
        default_model: "moonshot-v1-32k",
        input_cents_per_mtok: 100,
        output_cents_per_mtok: 300,
    },
    Preset {
        type_name: "minimax",
        base_url: "https://api.minimax.io/v1",
        default_model: "MiniMax-Text-01",
        input_cents_per_mtok: 30,
        output_cents_per_mtok: 120,
    },
    Preset {
        type_name: "together",
        base_url: "https://api.together.xyz/v1",
        default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        input_cents_per_mtok: 0,
        output_cents_per_mtok: 0,
    },
    Preset {
        type_name: "xiaomi",
        base_url: "https://api.xiaomimimo.com/v1",
        default_model: "mimo-v2-flash",
        input_cents_per_mtok: 30,
        output_cents_per_mtok: 120,
    },
    Preset {
        type_name: "opencode",
        base_url: "https://opencode.ai/zen/v1",
        default_model: "claude-sonnet-4-5",
        input_cents_per_mtok: 0,
        output_cents_per_mtok: 0,
    },
    Preset {
        type_name: "commandcode",
        base_url: "https://api.commandcode.ai/provider/v1",
        default_model: "deepseek/deepseek-v4-flash",
        input_cents_per_mtok: 0,
        output_cents_per_mtok: 0,
    },
];

/// Looks up a preset by its config `type` name.
#[must_use]
pub fn find(type_name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.type_name == type_name)
}

/// A configured provider backed by a [`Preset`] (OpenAI wire format).
pub struct PresetProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    descriptor: ModelDescriptor,
}

impl PresetProvider {
    /// Builds a provider from a preset, applying any config overrides for
    /// `base_url`, `model`, and `tier`.
    pub fn new(
        preset: &Preset,
        alias: &str,
        entry: &ProviderEntry,
        secrets: &SecretResolver,
    ) -> Result<Self, BoxError> {
        let api_key = resolve_api_key(entry, secrets)?;
        let model = entry
            .model
            .clone()
            .unwrap_or_else(|| preset.default_model.to_owned());
        let base_url = entry
            .base_url
            .clone()
            .unwrap_or_else(|| preset.base_url.to_owned());
        let tier = entry.tier.unwrap_or(Tier::Premium);
        Ok(Self {
            client: Client::new(),
            base_url,
            api_key,
            model,
            descriptor: ModelDescriptor {
                alias: alias.to_owned(),
                tier,
                input_price: Money::new("USD", preset.input_cents_per_mtok),
                output_price: Money::new("USD", preset.output_cents_per_mtok),
            },
        })
    }
}

#[async_trait]
impl Model for PresetProvider {
    fn descriptor(&self) -> ModelDescriptor {
        self.descriptor.clone()
    }

    async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ModelError> {
        openai_chat_complete(
            &self.client,
            &self.base_url,
            &self.api_key,
            &self.model,
            &req,
        )
        .await
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>, ModelError> {
        Err(ModelError::InvalidRequest(
            "embeddings not supported by this provider".to_owned(),
        ))
    }
}
