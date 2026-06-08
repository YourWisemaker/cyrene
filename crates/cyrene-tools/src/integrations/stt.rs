//! Speech-to-text (voice → text) integration tool.
//!
//! Transcribes a voice note into text so a Telegram/WhatsApp voice message can
//! flow through the agent as a normal text request (R7). Backed by ElevenLabs
//! Scribe by default, or OpenAI Whisper when `provider = "openai"` is set in the
//! tool options. The Channel_Gateway calls this with the audio URL of an
//! [`Attachment`](cyrene_core::Attachment) it received; the runtime fetches the
//! bytes and performs the provider HTTP call.

use cyrene_core::Risk;

use super::ToolSettings;
use crate::error::ToolError;
use crate::tool::{Tool, ToolOutput};

/// Default ElevenLabs speech-to-text endpoint.
const ELEVENLABS_STT_URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";
/// Default OpenAI transcription endpoint.
const OPENAI_STT_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

/// A configured speech-to-text transcriber (R28).
pub struct SpeechToTextTool {
    alias: String,
    provider: SttProvider,
    endpoint: String,
    model: String,
    #[allow(dead_code)] // Consumed by the runtime that performs the HTTP call.
    api_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SttProvider {
    ElevenLabs,
    OpenAi,
}

impl SpeechToTextTool {
    /// Builds the transcriber from its configured settings.
    ///
    /// # Errors
    /// Returns [`ToolError::InvalidArgs`] if the API key is missing or the
    /// `provider` option is unrecognized.
    pub fn new(alias: &str, settings: &ToolSettings) -> Result<Self, ToolError> {
        let api_key = settings.require_api_key("stt.transcribe")?.to_owned();
        let provider = match settings.option("provider").unwrap_or("elevenlabs") {
            "elevenlabs" => SttProvider::ElevenLabs,
            "openai" | "whisper" => SttProvider::OpenAi,
            other => {
                return Err(ToolError::InvalidArgs(format!(
                    "unknown stt provider `{other}` (use `elevenlabs` or `openai`)"
                )))
            }
        };
        let default_endpoint = match provider {
            SttProvider::ElevenLabs => ELEVENLABS_STT_URL,
            SttProvider::OpenAi => OPENAI_STT_URL,
        };
        let endpoint = settings
            .base_url
            .clone()
            .unwrap_or_else(|| default_endpoint.to_owned());
        let default_model = match provider {
            SttProvider::ElevenLabs => "scribe_v1",
            SttProvider::OpenAi => "whisper-1",
        };
        let model = settings.option("model").unwrap_or(default_model).to_owned();
        Ok(Self {
            alias: alias.to_owned(),
            provider,
            endpoint,
            model,
            api_key,
        })
    }
}

impl Tool for SpeechToTextTool {
    fn name(&self) -> &str {
        "stt.transcribe"
    }

    fn default_risk(&self) -> Risk {
        // Read-only: turns an audio asset into text, no external mutation.
        Risk::Low
    }

    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        // Accept either a fetchable URL or already-fetched base64 audio bytes.
        let source = args
            .get("audio_url")
            .or_else(|| args.get("audio_base64"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgs("missing `audio_url` or `audio_base64`".to_owned())
            })?;
        if source.is_empty() {
            return Err(ToolError::InvalidArgs("empty audio source".to_owned()));
        }
        let provider = match self.provider {
            SttProvider::ElevenLabs => "elevenlabs",
            SttProvider::OpenAi => "openai",
        };
        Ok(ToolOutput::ok(format!(
            "[stt.transcribe:{}] transcribe via {provider} model {} ({})",
            self.alias, self.model, self.endpoint
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn tool_with(provider: Option<&str>) -> SpeechToTextTool {
        let mut options = BTreeMap::new();
        if let Some(p) = provider {
            options.insert("provider".to_owned(), p.to_owned());
        }
        SpeechToTextTool::new(
            "voice",
            &ToolSettings {
                api_key: Some("el_fake".to_owned()),
                options,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn name_is_stable() {
        assert_eq!(tool_with(None).name(), "stt.transcribe");
    }

    #[test]
    fn requires_api_key() {
        assert!(SpeechToTextTool::new("voice", &ToolSettings::default()).is_err());
    }

    #[test]
    fn defaults_to_elevenlabs() {
        let out = tool_with(None)
            .run(&json!({ "audio_url": "https://x/voice.ogg" }))
            .unwrap();
        assert!(out.text.contains("elevenlabs"));
        assert!(out.text.contains("scribe_v1"));
    }

    #[test]
    fn openai_provider_selects_whisper() {
        let out = tool_with(Some("openai"))
            .run(&json!({ "audio_url": "https://x/voice.ogg" }))
            .unwrap();
        assert!(out.text.contains("openai"));
        assert!(out.text.contains("whisper-1"));
    }

    #[test]
    fn unknown_provider_errors() {
        assert!(SpeechToTextTool::new(
            "voice",
            &ToolSettings {
                api_key: Some("k".to_owned()),
                options: BTreeMap::from([("provider".to_owned(), "acme".to_owned())]),
                ..Default::default()
            },
        )
        .is_err());
    }

    #[test]
    fn missing_audio_source_errors() {
        let err = tool_with(None).run(&json!({})).unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn transcribe_is_low_risk() {
        assert_eq!(tool_with(None).default_risk(), Risk::Low);
    }
}
