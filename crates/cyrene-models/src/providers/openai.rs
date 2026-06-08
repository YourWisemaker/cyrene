//! OpenAI provider + shared helpers for OpenAI-compatible APIs.

use async_trait::async_trait;
use cyrene_config::{BoxError, ProviderEntry, SecretResolver};
use cyrene_core::{
    FinishReason, Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Money, Role,
    Tier, TokenUsage,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Resolve an API key from the secret resolver, erroring if missing.
pub(crate) fn resolve_api_key(
    entry: &ProviderEntry,
    secrets: &SecretResolver,
) -> Result<String, BoxError> {
    let env_name = entry
        .api_key_env
        .as_deref()
        .ok_or("provider requires api_key_env to be configured")?;
    secrets
        .require(env_name)
        .map_err(|e| Box::new(e) as BoxError)
}

/// Classify an HTTP/reqwest error into a ModelError.
pub(crate) fn classify_http_error(err: reqwest::Error) -> ModelError {
    if err.is_timeout() {
        ModelError::Timeout(err.to_string())
    } else if err.is_connect() {
        ModelError::Unavailable(err.to_string())
    } else {
        ModelError::Provider(err.to_string())
    }
}

// ─── OpenAI wire format (shared by openai_compat and openrouter) ─────────────

#[derive(Serialize)]
pub(crate) struct OpenAiRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct OpenAiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiResponse {
    pub choices: Vec<OpenAiChoice>,
    pub usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiChoice {
    pub message: OpenAiRespMessage,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiRespMessage {
    pub content: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct OpenAiUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

pub(crate) fn build_openai_request(model: &str, req: &ModelRequest) -> OpenAiRequest {
    OpenAiRequest {
        model: model.to_owned(),
        messages: req
            .messages
            .iter()
            .map(|m| OpenAiMessage {
                role: match m.role {
                    Role::System => "system".to_owned(),
                    Role::User => "user".to_owned(),
                    Role::Assistant => "assistant".to_owned(),
                    Role::Tool => "tool".to_owned(),
                },
                content: m.content.clone(),
            })
            .collect(),
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        stop: req.stop.clone(),
    }
}

/// Runs an OpenAI-style `chat/completions` call against `base_url` and parses
/// the response. Shared by every OpenAI-compatible provider (DeepSeek, Groq,
/// xAI, Mistral, Together, OpenRouter, …) so the wire plumbing lives in one
/// place.
pub(crate) async fn openai_chat_complete(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    req: &ModelRequest,
) -> Result<ModelResponse, ModelError> {
    let body = build_openai_request(model, req);
    let url = format!("{base_url}/chat/completions");
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
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
    let data: OpenAiResponse = resp.json().await.map_err(classify_http_error)?;
    Ok(parse_openai_response(data))
}

pub(crate) fn parse_openai_response(resp: OpenAiResponse) -> ModelResponse {
    let choice = resp.choices.into_iter().next().unwrap_or(OpenAiChoice {
        message: OpenAiRespMessage {
            content: Some(String::new()),
        },
        finish_reason: Some("stop".to_owned()),
    });
    let usage = resp.usage.unwrap_or(OpenAiUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
    });
    let finish_reason = match choice.finish_reason.as_deref() {
        Some("stop") | None => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("content_filter") => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    };
    ModelResponse::new(
        choice.message.content.unwrap_or_default(),
        TokenUsage::new(usage.prompt_tokens, usage.completion_tokens),
        finish_reason,
    )
}

// ─── OpenAI Provider ─────────────────────────────────────────────────────────

pub struct OpenAiProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    descriptor: ModelDescriptor,
}

impl OpenAiProvider {
    pub fn new(
        alias: &str,
        entry: &ProviderEntry,
        secrets: &SecretResolver,
    ) -> Result<Self, BoxError> {
        let api_key = resolve_api_key(entry, secrets)?;
        let model = entry.model.clone().unwrap_or_else(|| "gpt-4o".to_owned());
        let base_url = entry
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let tier = entry.tier.unwrap_or(Tier::Premium);
        Ok(Self {
            client: Client::new(),
            base_url,
            api_key,
            model,
            descriptor: ModelDescriptor {
                alias: alias.to_owned(),
                tier,
                input_price: Money::new("USD", 500), // $5/1M tokens
                output_price: Money::new("USD", 1500), // $15/1M tokens
            },
        })
    }
}

#[async_trait]
impl Model for OpenAiProvider {
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
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.as_u16() == 429 {
                return Err(ModelError::RateLimited(text));
            }
            return Err(ModelError::Provider(format!("{status}: {text}")));
        }
        let data: OpenAiResponse = resp.json().await.map_err(classify_http_error)?;
        Ok(parse_openai_response(data))
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>, ModelError> {
        let url = format!("{}/embeddings", self.base_url);
        let body = serde_json::json!({
            "model": "text-embedding-3-small",
            "input": text,
        });
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
        #[derive(Deserialize)]
        struct EmbResp {
            data: Vec<EmbData>,
        }
        #[derive(Deserialize)]
        struct EmbData {
            embedding: Vec<f32>,
        }
        let data: EmbResp = resp.json().await.map_err(classify_http_error)?;
        Ok(data
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .unwrap_or_default())
    }
}
