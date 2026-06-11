//! OpenRouter provider (200+ models behind one key, OpenAI-compatible wire format).

use async_trait::async_trait;
use cyrene_config::{BoxError, ProviderEntry, SecretResolver};
use cyrene_core::{Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Money, Tier};
use reqwest::Client;

use super::openai::{
    build_openai_request, classify_http_error, parse_openai_response, resolve_api_key,
    OpenAiResponse,
};

const BASE_URL: &str = "https://openrouter.ai/api/v1";

pub struct OpenRouterProvider {
    client: Client,
    api_key: String,
    model: String,
    descriptor: ModelDescriptor,
}

impl OpenRouterProvider {
    pub fn new(
        alias: &str,
        entry: &ProviderEntry,
        secrets: &SecretResolver,
    ) -> Result<Self, BoxError> {
        let api_key = resolve_api_key(entry, secrets)?;
        let model = entry
            .model
            .clone()
            .unwrap_or_else(|| "anthropic/claude-3.5-sonnet".to_owned());
        let tier = entry.tier.unwrap_or(Tier::Premium);
        Ok(Self {
            client: Client::new(),
            api_key,
            model,
            descriptor: ModelDescriptor {
                alias: alias.to_owned(),
                tier,
                input_price: Money::zero("USD"),
                output_price: Money::zero("USD"),
            },
        })
    }
}

#[async_trait]
impl Model for OpenRouterProvider {
    fn descriptor(&self) -> ModelDescriptor {
        self.descriptor.clone()
    }

    async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ModelError> {
        let body = build_openai_request(&self.model, &req);
        let url = format!("{BASE_URL}/chat/completions");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(classify_http_error)?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ModelError::Provider(text));
        }
        let data: OpenAiResponse = resp.json().await.map_err(classify_http_error)?;
        Ok(parse_openai_response(data))
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>, ModelError> {
        Err(ModelError::InvalidRequest(
            "embeddings not directly supported via OpenRouter".to_owned(),
        ))
    }

    async fn list_models(&self) -> Result<Vec<String>, ModelError> {
        super::openai::openai_list_models(&self.client, BASE_URL, &self.api_key).await
    }
}
