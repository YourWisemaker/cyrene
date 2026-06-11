//! Generic OpenAI-compatible provider (DeepSeek, xAI, Groq, self-hosted).

use async_trait::async_trait;
use cyrene_config::{BoxError, ProviderEntry, SecretResolver};
use cyrene_core::{Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Money, Tier};
use reqwest::Client;

use super::openai::{
    build_openai_request, classify_http_error, parse_openai_response, resolve_api_key,
    OpenAiResponse,
};

pub struct OpenAiCompatProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    descriptor: ModelDescriptor,
}

impl OpenAiCompatProvider {
    pub fn new(
        alias: &str,
        entry: &ProviderEntry,
        secrets: &SecretResolver,
    ) -> Result<Self, BoxError> {
        let api_key = resolve_api_key(entry, secrets)?;
        let base_url = entry
            .base_url
            .clone()
            .ok_or("openai_compat provider requires base_url")?;
        let model = entry.model.clone().unwrap_or_else(|| "default".to_owned());
        let tier = entry.tier.unwrap_or(Tier::Premium);
        Ok(Self {
            client: Client::new(),
            base_url,
            api_key,
            model,
            descriptor: ModelDescriptor {
                alias: alias.to_owned(),
                tier,
                input_price: Money::new("USD", 100),
                output_price: Money::new("USD", 300),
            },
        })
    }
}

#[async_trait]
impl Model for OpenAiCompatProvider {
    fn descriptor(&self) -> ModelDescriptor {
        self.descriptor.clone()
    }

    async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ModelError> {
        let body = build_openai_request(&self.model, &req);
        let url = format!("{}/chat/completions", self.base_url);
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
            "embeddings not supported by this provider".to_owned(),
        ))
    }

    async fn list_models(&self) -> Result<Vec<String>, ModelError> {
        super::openai::openai_list_models(&self.client, &self.base_url, &self.api_key).await
    }
}
