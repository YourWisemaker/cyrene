//! Google Gemini provider.

use async_trait::async_trait;
use cyrene_config::{BoxError, ProviderEntry, SecretResolver};
use cyrene_core::{
    FinishReason, Model, ModelDescriptor, ModelError, ModelRequest, ModelResponse, Money, Role,
    Tier, TokenUsage,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::openai::{classify_http_error, resolve_api_key};

pub struct GeminiProvider {
    client: Client,
    api_key: String,
    model: String,
    descriptor: ModelDescriptor,
}

impl GeminiProvider {
    pub fn new(
        alias: &str,
        entry: &ProviderEntry,
        secrets: &SecretResolver,
    ) -> Result<Self, BoxError> {
        let api_key = resolve_api_key(entry, secrets)?;
        let model = entry
            .model
            .clone()
            .unwrap_or_else(|| "gemini-1.5-pro".to_owned());
        let tier = entry.tier.unwrap_or(Tier::Premium);
        Ok(Self {
            client: Client::new(),
            api_key,
            model,
            descriptor: ModelDescriptor {
                alias: alias.to_owned(),
                tier,
                input_price: Money::new("USD", 125),  // $1.25/1M
                output_price: Money::new("USD", 500), // $5/1M
            },
        })
    }
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
}
#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}
#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
}
#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiRespContent>,
}
#[derive(Deserialize)]
struct GeminiRespContent {
    parts: Option<Vec<GeminiRespPart>>,
}
#[derive(Deserialize)]
struct GeminiRespPart {
    text: Option<String>,
}
#[derive(Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u64>,
}

#[async_trait]
impl Model for GeminiProvider {
    fn descriptor(&self) -> ModelDescriptor {
        self.descriptor.clone()
    }

    async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ModelError> {
        let contents: Vec<GeminiContent> = req
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| GeminiContent {
                role: if m.role == Role::User {
                    "user".to_owned()
                } else {
                    "model".to_owned()
                },
                parts: vec![GeminiPart {
                    text: m.content.clone(),
                }],
            })
            .collect();
        let body = GeminiRequest { contents };
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );
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
        let data: GeminiResponse = resp.json().await.map_err(classify_http_error)?;
        let text = data
            .candidates
            .and_then(|c| c.into_iter().next())
            .and_then(|c| c.content)
            .and_then(|c| c.parts)
            .and_then(|p| p.into_iter().next())
            .and_then(|p| p.text)
            .unwrap_or_default();
        let usage = data.usage_metadata.unwrap_or(GeminiUsage {
            prompt_token_count: Some(0),
            candidates_token_count: Some(0),
        });
        Ok(ModelResponse::new(
            text,
            TokenUsage::new(
                usage.prompt_token_count.unwrap_or(0),
                usage.candidates_token_count.unwrap_or(0),
            ),
            FinishReason::Stop,
        ))
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>, ModelError> {
        Err(ModelError::InvalidRequest(
            "Gemini embeddings not yet implemented".to_owned(),
        ))
    }
}
