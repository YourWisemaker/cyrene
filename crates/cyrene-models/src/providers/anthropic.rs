//! Anthropic (Claude) provider — uses the Messages API.

use async_trait::async_trait;
use cyrene_config::{BoxError, ProviderEntry, SecretResolver};
use cyrene_core::{
    FinishReason, Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Money, Role,
    Tier, TokenUsage,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::openai::{classify_http_error, resolve_api_key};

const BASE_URL: &str = "https://api.anthropic.com/v1";

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    descriptor: ModelDescriptor,
}

impl AnthropicProvider {
    pub fn new(
        alias: &str,
        entry: &ProviderEntry,
        secrets: &SecretResolver,
    ) -> Result<Self, BoxError> {
        let api_key = resolve_api_key(entry, secrets)?;
        let model = entry
            .model
            .clone()
            .unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_owned());
        let tier = entry.tier.unwrap_or(Tier::Premium);
        Ok(Self {
            client: Client::new(),
            api_key,
            model,
            descriptor: ModelDescriptor {
                alias: alias.to_owned(),
                tier,
                input_price: Money::new("USD", 300),   // $3/1M
                output_price: Money::new("USD", 1500), // $15/1M
            },
        })
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    usage: AnthropicUsage,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
}

#[async_trait]
impl Model for AnthropicProvider {
    fn descriptor(&self) -> ModelDescriptor {
        self.descriptor.clone()
    }

    async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ModelError> {
        let mut system = None;
        let mut messages = Vec::new();
        for m in &req.messages {
            match m.role {
                Role::System => system = Some(m.content.clone()),
                Role::User => messages.push(AnthropicMessage {
                    role: "user".to_owned(),
                    content: m.content.clone(),
                }),
                Role::Assistant => messages.push(AnthropicMessage {
                    role: "assistant".to_owned(),
                    content: m.content.clone(),
                }),
                Role::Tool => messages.push(AnthropicMessage {
                    role: "user".to_owned(),
                    content: m.content.clone(),
                }),
            }
        }
        let body = AnthropicRequest {
            model: self.model.clone(),
            messages,
            max_tokens: req.max_tokens.unwrap_or(4096),
            system,
        };
        let url = format!("{BASE_URL}/messages");
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(classify_http_error)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.as_u16() == 429 {
                return Err(ModelError::RateLimited(text));
            }
            return Err(ModelError::Provider(format!("{status}: {text}")));
        }
        let data: AnthropicResponse = resp.json().await.map_err(classify_http_error)?;
        let content = data
            .content
            .into_iter()
            .filter_map(|b| b.text)
            .collect::<Vec<_>>()
            .join("");
        let finish = match data.stop_reason.as_deref() {
            Some("end_turn") | Some("stop") | None => FinishReason::Stop,
            Some("max_tokens") => FinishReason::Length,
            _ => FinishReason::Other,
        };
        Ok(ModelResponse::new(
            content,
            TokenUsage::new(data.usage.input_tokens, data.usage.output_tokens),
            finish,
        ))
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>, ModelError> {
        Err(ModelError::InvalidRequest(
            "Anthropic does not provide an embeddings API".to_owned(),
        ))
    }
}
