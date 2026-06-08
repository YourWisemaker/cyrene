//! Model provider implementations.

pub mod anthropic;
pub mod gemini;
pub mod ollama;
pub mod openai;
pub mod openai_compat;
pub mod openrouter;
pub mod presets;

pub use anthropic::AnthropicProvider;
pub use gemini::GeminiProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use openai_compat::OpenAiCompatProvider;
pub use openrouter::OpenRouterProvider;
pub use presets::{PresetProvider, PRESETS};
