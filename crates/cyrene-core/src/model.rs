//! The [`Model`] trait and its request/response/descriptor types.
//!
//! A [`Model`] is a single configured Model_Provider endpoint (local or
//! premium). The Model_Router (task 8) selects among registered models per
//! step, escalating on repeated failure (R12), and the Budget_Guard meters the
//! [`TokenUsage`] every [`ModelResponse`] reports (R13). Providers describe
//! their [`Tier`] and pricing through a [`ModelDescriptor`] so the router can
//! reason about cost without provider-specific knowledge (R2.3, R12.2).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Recoverability, Recoverable};
use crate::money::Money;

/// The cost/capability tier a [`Model`] belongs to.
///
/// The router defaults to a [`Tier::Local`] provider and only escalates to a
/// [`Tier::Premium`] one after repeated failure (R12.1, R12.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tier {
    /// A local provider (e.g. Ollama): cheap/free, the default selection.
    Local,
    /// A premium cloud provider (e.g. Anthropic, OpenAI): used on escalation.
    Premium,
}

/// Static, provider-reported metadata about a [`Model`].
///
/// Prices are expressed as the [`Money`] cost per
/// [`ModelDescriptor::PRICE_UNIT_TOKENS`] tokens, which keeps per-token costs
/// (which are fractions of a cent) representable as exact integer minor units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDescriptor {
    /// The configured alias the Plugin_Registry keys this provider by.
    pub alias: String,
    /// The provider's cost/capability tier.
    pub tier: Tier,
    /// Price per [`ModelDescriptor::PRICE_UNIT_TOKENS`] input (prompt) tokens.
    pub input_price: Money,
    /// Price per [`ModelDescriptor::PRICE_UNIT_TOKENS`] output (completion)
    /// tokens.
    pub output_price: Money,
}

impl ModelDescriptor {
    /// The token count the [`ModelDescriptor::input_price`] and
    /// [`ModelDescriptor::output_price`] are quoted against.
    pub const PRICE_UNIT_TOKENS: u64 = 1_000_000;

    /// Computes the monetary cost of the given [`TokenUsage`] under this
    /// descriptor's pricing, for the Budget_Guard to meter (R13.2).
    ///
    /// # Errors
    /// Returns [`CoreError::CurrencyMismatch`] if the input and output prices
    /// use different currencies, or [`CoreError::Overflow`] if the cost
    /// arithmetic overflows `i64`.
    pub fn cost_of(&self, usage: &TokenUsage) -> Result<Money, CoreError> {
        let input = scale_price(&self.input_price, usage.input_tokens)?;
        let output = scale_price(&self.output_price, usage.output_tokens)?;
        input.checked_add(&output)
    }
}

/// Multiplies a per-[`ModelDescriptor::PRICE_UNIT_TOKENS`] price by a token
/// count, rounding down to the nearest minor unit.
fn scale_price(price: &Money, tokens: u64) -> Result<Money, CoreError> {
    let tokens =
        i64::try_from(tokens).map_err(|_| CoreError::Overflow("ModelDescriptor::cost_of"))?;
    let scaled = price
        .minor_units
        .checked_mul(tokens)
        .ok_or(CoreError::Overflow("ModelDescriptor::cost_of"))?;
    let unit = i64::try_from(ModelDescriptor::PRICE_UNIT_TOKENS)
        .map_err(|_| CoreError::Overflow("ModelDescriptor::cost_of"))?;
    Ok(Money::new(price.currency.clone(), scaled / unit))
}

/// The role a [`ChatMessage`] plays in a [`ModelRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// A system/developer instruction that frames the conversation.
    System,
    /// Input authored by the user.
    User,
    /// A prior response produced by the model.
    Assistant,
    /// Output from a tool, fed back in for the model to reason over.
    Tool,
}

/// A single message in a chat-style [`ModelRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Who authored this message.
    pub role: Role,
    /// The message text.
    pub content: String,
}

impl ChatMessage {
    /// Creates a message with the given role and content.
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }

    /// Convenience constructor for a [`Role::System`] message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }

    /// Convenience constructor for a [`Role::User`] message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }

    /// Convenience constructor for a [`Role::Assistant`] message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }
}

/// A completion request sent to a [`Model`].
///
/// Kept provider-agnostic: concrete providers translate this into their own
/// wire format. Sampling controls are optional so a provider falls back to its
/// own defaults when they are absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    /// The ordered conversation to complete from.
    pub messages: Vec<ChatMessage>,
    /// Optional cap on the number of tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Optional sampling temperature (provider-defined range).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Optional stop sequences that end generation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
}

impl ModelRequest {
    /// Creates a request from a conversation, leaving sampling controls unset.
    #[must_use]
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            max_tokens: None,
            temperature: None,
            stop: Vec::new(),
        }
    }
}

/// The token accounting a provider reports for one completion.
///
/// This is the unit the Budget_Guard meters against a session's token and cost
/// caps (R13.2, R13.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Tokens consumed by the prompt/input.
    pub input_tokens: u64,
    /// Tokens produced in the completion/output.
    pub output_tokens: u64,
}

impl TokenUsage {
    /// Creates a usage record.
    #[must_use]
    pub fn new(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens,
            output_tokens,
        }
    }

    /// Returns the total tokens (input + output), saturating on overflow.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

/// Why a [`Model`] stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Generation reached a natural stop or a stop sequence.
    Stop,
    /// Generation hit the token limit.
    Length,
    /// The provider filtered the content.
    ContentFilter,
    /// Some other provider-specific reason.
    Other,
}

/// A completion produced by a [`Model`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelResponse {
    /// The generated text.
    pub content: String,
    /// Token accounting for this completion, for budget metering.
    pub usage: TokenUsage,
    /// Why generation stopped.
    pub finish_reason: FinishReason,
}

impl ModelResponse {
    /// Creates a response with the given content, usage, and finish reason.
    pub fn new(content: impl Into<String>, usage: TokenUsage, finish_reason: FinishReason) -> Self {
        Self {
            content: content.into(),
            usage,
            finish_reason,
        }
    }
}

/// Errors a [`Model`] implementation can return.
///
/// The [`Recoverable`] hint drives the Model_Router: provider/availability
/// failures escalate to a premium provider (R12.4), transient failures retry,
/// and request/context errors halt.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// The provider rejected the request as malformed or unsupported.
    #[error("invalid model request: {0}")]
    InvalidRequest(String),

    /// The request (prompt + expected output) exceeds the model's context.
    #[error("context length exceeded: {0}")]
    ContextLengthExceeded(String),

    /// The provider returned an error or behaved unexpectedly.
    #[error("model provider error: {0}")]
    Provider(String),

    /// The provider endpoint is unreachable or not configured.
    #[error("model provider unavailable: {0}")]
    Unavailable(String),

    /// The provider rate-limited the request.
    #[error("model provider rate limited: {0}")]
    RateLimited(String),

    /// The request timed out.
    #[error("model request timed out: {0}")]
    Timeout(String),

    /// A request or response payload failed to (de)serialize.
    #[error("model serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl Recoverable for ModelError {
    fn recoverability(&self) -> Recoverability {
        match self {
            // Provider/availability failures are what the router escalates on.
            Self::Provider(_) | Self::Unavailable(_) => Recoverability::Escalate,
            // Transient conditions are worth retrying as-is.
            Self::RateLimited(_) | Self::Timeout(_) => Recoverability::Retry,
            // Request-shape problems will not fix themselves.
            Self::InvalidRequest(_) | Self::ContextLengthExceeded(_) | Self::Serialization(_) => {
                Recoverability::Halt
            }
        }
    }
}

/// A configured Model_Provider endpoint.
///
/// Implementations are registered in the Plugin_Registry by their
/// [`ModelDescriptor::alias`] and selected by the Model_Router (R2.1, R2.3).
#[async_trait]
pub trait Model: Send + Sync {
    /// Returns this provider's static descriptor (tier + pricing).
    fn descriptor(&self) -> ModelDescriptor;

    /// Produces a completion for the given request.
    ///
    /// # Errors
    /// Returns a [`ModelError`] whose [`Recoverable`] hint tells the router
    /// whether to retry, escalate, or halt.
    async fn complete(&self, req: ModelRequest) -> Result<ModelResponse, ModelError>;

    /// Lists the model identifiers this provider advertises (e.g. via the
    /// OpenAI `/v1/models` endpoint or Ollama `/api/tags`).
    ///
    /// The default returns an empty list, meaning discovery is unsupported and
    /// callers should fall back to a manually entered model name.
    ///
    /// # Errors
    /// Returns a [`ModelError`] if the provider supports discovery but the
    /// request fails.
    async fn list_models(&self) -> Result<Vec<String>, ModelError> {
        Ok(Vec::new())
    }

    /// Returns an embedding vector for the given text.
    ///
    /// # Errors
    /// Returns a [`ModelError`] if the provider cannot produce an embedding.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, ModelError>;
}

#[cfg(test)]
mod tests {
    use super::{
        ChatMessage, FinishReason, ModelDescriptor, ModelRequest, ModelResponse, Role, Tier,
        TokenUsage,
    };
    use crate::error::CoreError;
    use crate::money::Money;

    /// Builds a descriptor priced per [`ModelDescriptor::PRICE_UNIT_TOKENS`].
    fn descriptor(input: i64, output: i64) -> ModelDescriptor {
        ModelDescriptor {
            alias: "test-model".to_owned(),
            tier: Tier::Premium,
            input_price: Money::new("USD", input),
            output_price: Money::new("USD", output),
        }
    }

    #[test]
    fn token_usage_total_sums_input_and_output() {
        let usage = TokenUsage::new(120, 80);
        assert_eq!(usage.total(), 200);
    }

    #[test]
    fn token_usage_total_saturates_on_overflow() {
        let usage = TokenUsage::new(u64::MAX, 1);
        assert_eq!(usage.total(), u64::MAX);
    }

    #[test]
    fn cost_of_computes_expected_cost() {
        // $3 / 1M input tokens, $15 / 1M output tokens.
        let desc = descriptor(300, 1500);
        // Exactly one price-unit of each: 300 + 1500 minor units = $18.00.
        let usage = TokenUsage::new(
            ModelDescriptor::PRICE_UNIT_TOKENS,
            ModelDescriptor::PRICE_UNIT_TOKENS,
        );
        assert_eq!(desc.cost_of(&usage).unwrap(), Money::new("USD", 1800));
    }

    #[test]
    fn cost_of_scales_with_token_count() {
        let desc = descriptor(300, 1500);
        // Half a price-unit of input, two price-units of output.
        let usage = TokenUsage::new(
            ModelDescriptor::PRICE_UNIT_TOKENS / 2,
            ModelDescriptor::PRICE_UNIT_TOKENS * 2,
        );
        // input: 300 * 500_000 / 1_000_000 = 150
        // output: 1500 * 2_000_000 / 1_000_000 = 3000
        assert_eq!(desc.cost_of(&usage).unwrap(), Money::new("USD", 3150));
    }

    #[test]
    fn cost_of_rounds_down_to_nearest_minor_unit() {
        // 300 minor units / 1M tokens, only 5000 tokens:
        // 300 * 5000 = 1_500_000; / 1_000_000 = 1 (1.5 rounded down).
        let desc = descriptor(300, 0);
        let usage = TokenUsage::new(5000, 0);
        assert_eq!(desc.cost_of(&usage).unwrap(), Money::new("USD", 1));

        // Below one minor unit rounds down to zero rather than erroring.
        let usage = TokenUsage::new(3000, 0);
        // 300 * 3000 = 900_000; / 1_000_000 = 0.
        assert_eq!(desc.cost_of(&usage).unwrap(), Money::new("USD", 0));
    }

    #[test]
    fn cost_of_zero_usage_is_zero() {
        let desc = descriptor(300, 1500);
        let usage = TokenUsage::default();
        assert_eq!(desc.cost_of(&usage).unwrap(), Money::new("USD", 0));
    }

    #[test]
    fn cost_of_currency_mismatch_between_prices_errors() {
        let desc = ModelDescriptor {
            alias: "mixed".to_owned(),
            tier: Tier::Premium,
            input_price: Money::new("USD", 300),
            output_price: Money::new("EUR", 1500),
        };
        let usage = TokenUsage::new(
            ModelDescriptor::PRICE_UNIT_TOKENS,
            ModelDescriptor::PRICE_UNIT_TOKENS,
        );
        assert!(matches!(
            desc.cost_of(&usage),
            Err(CoreError::CurrencyMismatch { .. })
        ));
    }

    #[test]
    fn cost_of_overflow_errors() {
        let desc = descriptor(i64::MAX, 0);
        // input_price.minor_units * tokens overflows i64.
        let usage = TokenUsage::new(ModelDescriptor::PRICE_UNIT_TOKENS, 0);
        assert!(matches!(desc.cost_of(&usage), Err(CoreError::Overflow(_))));
    }

    #[test]
    fn tier_round_trip_each_variant() {
        for tier in [Tier::Local, Tier::Premium] {
            let json = serde_json::to_string(&tier).unwrap();
            let back: Tier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, back);
        }
    }

    #[test]
    fn model_descriptor_round_trip() {
        let desc = descriptor(300, 1500);
        let json = serde_json::to_string(&desc).unwrap();
        let back: ModelDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(desc, back);
    }

    #[test]
    fn token_usage_round_trip() {
        let usage = TokenUsage::new(123, 456);
        let json = serde_json::to_string(&usage).unwrap();
        let back: TokenUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
    }

    #[test]
    fn role_round_trip_each_variant() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let back: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
        // Roles serialize in lowercase per the `rename_all` attribute.
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), "\"system\"");
    }

    #[test]
    fn finish_reason_round_trip_each_variant() {
        for reason in [
            FinishReason::Stop,
            FinishReason::Length,
            FinishReason::ContentFilter,
            FinishReason::Other,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let back: FinishReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, back);
        }
        // snake_case rename for multi-word variants.
        assert_eq!(
            serde_json::to_string(&FinishReason::ContentFilter).unwrap(),
            "\"content_filter\""
        );
    }

    #[test]
    fn model_request_round_trip_skips_empty_optionals() {
        let req = ModelRequest::new(vec![
            ChatMessage::system("be brief"),
            ChatMessage::user("hello"),
        ]);
        let json = serde_json::to_string(&req).unwrap();
        // Unset sampling controls and empty stop list are omitted from the wire form.
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("stop"));
        let back: ModelRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn model_response_round_trip() {
        let resp = ModelResponse::new("done", TokenUsage::new(10, 20), FinishReason::Stop);
        let json = serde_json::to_string(&resp).unwrap();
        let back: ModelResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }
}
