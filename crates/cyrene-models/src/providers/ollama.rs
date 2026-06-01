//! Local Ollama provider (OpenAI-compatible, no API key needed).

use async_trait::async_trait;
use cyrene_config::{BoxError, ProviderEntry};
use cyrene_core::{Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Money, Tier};
use reqwest::Client;

use super::openai::{
    build_openai_request, classify_http_error, parse_openai_response, OpenAiResponse,
};

const DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
    descriptor: ModelDescriptor,
}

impl OllamaProvider {
    pub fn new(alias: &str, entry: &ProviderEntry) -> Result<Self, BoxError> {
        let model = entry.model.clone().unwrap_or_else(|| "llama3.1".to_owned());
        let base_url = entry
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        Ok(Self {
            client: Client::new(),
            base_url,
            model,
            descriptor: ModelDescriptor {
                alias: alias.to_owned(),
                tier: Tier::Local,
                input_price: Money::zero("USD"),
                output_price: Money::zero("USD"),
            },
        })
    }
}

#[async_trait]
impl Model for OllamaProvider {
    fn descriptor(&self) -> ModelDescriptor {
        self.descriptor.clone()
    }

    async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ModelError> {
        let body = build_openai_request(&self.model, &req);
        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
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

    async fn embed(&self, text: &str) -> Result<Vec<f32>, ModelError> {
        let url = format!("{}/embeddings", self.base_url);
        let body = serde_json::json!({"model": self.model, "input": text});
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(classify_http_error)?;
        if !resp.status().is_success() {
            let t = resp.text().await.unwrap_or_default();
            return Err(ModelError::Provider(t));
        }
        #[derive(serde::Deserialize)]
        struct R {
            data: Vec<D>,
        }
        #[derive(serde::Deserialize)]
        struct D {
            embedding: Vec<f32>,
        }
        let data: R = resp.json().await.map_err(classify_http_error)?;
        Ok(data
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .unwrap_or_default())
    }
}
