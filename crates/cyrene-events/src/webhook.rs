//! Webhook event listener with HMAC-SHA256 signature verification (R9).
//!
//! The event listener receives webhook payloads from GitHub, Stripe, Linear,
//! etc. Each source has a shared secret; the listener verifies the payload's
//! HMAC-SHA256 signature before processing. Invalid signatures are rejected and
//! recorded (R9.4). Valid events that match a configured trigger start a
//! session (R9.2).
//!
//! The listener is transport-agnostic: it processes a `(headers, body)` pair
//! and returns a verdict. The actual HTTP server (axum) binds at the CLI layer.

use sha2::{Digest, Sha256};

/// A configured webhook source with its shared secret and trigger filter.
#[derive(Debug, Clone)]
pub struct WebhookSource {
    /// A human-readable name for this source (e.g. `"github"`).
    pub name: String,
    /// The shared secret used to verify HMAC-SHA256 signatures.
    pub secret: Vec<u8>,
    /// The header name carrying the signature (e.g. `"x-hub-signature-256"`).
    pub signature_header: String,
    /// Event types that trigger a session (e.g. `["push", "pull_request"]`).
    /// An empty list means all events trigger.
    pub triggers: Vec<String>,
}

impl WebhookSource {
    /// Creates a source for GitHub webhooks.
    pub fn github(secret: impl Into<Vec<u8>>, triggers: Vec<String>) -> Self {
        Self {
            name: "github".to_owned(),
            secret: secret.into(),
            signature_header: "x-hub-signature-256".to_owned(),
            triggers,
        }
    }

    /// Creates a source for Stripe webhooks.
    pub fn stripe(secret: impl Into<Vec<u8>>, triggers: Vec<String>) -> Self {
        Self {
            name: "stripe".to_owned(),
            secret: secret.into(),
            signature_header: "stripe-signature".to_owned(),
            triggers,
        }
    }

    /// Creates a source for Linear webhooks.
    pub fn linear(secret: impl Into<Vec<u8>>, triggers: Vec<String>) -> Self {
        Self {
            name: "linear".to_owned(),
            secret: secret.into(),
            signature_header: "linear-signature".to_owned(),
            triggers,
        }
    }
}

/// The outcome of processing a webhook request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookVerdict {
    /// Signature valid and event matches a trigger — start a session.
    Triggered {
        /// The source name.
        source: String,
        /// The event type that matched.
        event_type: String,
    },
    /// Signature valid but the event type does not match any trigger.
    Ignored,
    /// Signature verification failed — reject and log (R9.4).
    Rejected {
        /// The source name.
        source: String,
        /// Why verification failed.
        reason: String,
    },
    /// No source matched the request (unknown webhook origin).
    Unknown,
}

/// The event listener: verifies and dispatches webhook payloads.
#[derive(Debug, Clone)]
pub struct EventListener {
    sources: Vec<WebhookSource>,
}

impl EventListener {
    /// Creates a listener with the given configured sources.
    pub fn new(sources: Vec<WebhookSource>) -> Self {
        Self { sources }
    }

    /// Processes a webhook request: finds the matching source by signature
    /// header presence, verifies the HMAC, and checks the trigger filter.
    ///
    /// `headers` is a slice of `(name, value)` pairs (lowercased names).
    /// `body` is the raw request body bytes.
    /// `event_type` is the event type extracted from the payload or a header
    /// (e.g. `x-github-event`).
    pub fn process(
        &self,
        headers: &[(&str, &str)],
        body: &[u8],
        event_type: &str,
    ) -> WebhookVerdict {
        for source in &self.sources {
            let sig_header = headers
                .iter()
                .find(|(name, _)| *name == source.signature_header);

            let Some((_, sig_value)) = sig_header else {
                continue;
            };

            // Verify HMAC-SHA256.
            let expected = hmac_sha256(&source.secret, body);
            let expected_hex = format!("sha256={}", hex_encode(&expected));

            if !constant_time_eq(sig_value.as_bytes(), expected_hex.as_bytes()) {
                return WebhookVerdict::Rejected {
                    source: source.name.clone(),
                    reason: "HMAC-SHA256 signature mismatch".to_owned(),
                };
            }

            // Signature valid — check trigger filter.
            if source.triggers.is_empty() || source.triggers.iter().any(|t| t == event_type) {
                return WebhookVerdict::Triggered {
                    source: source.name.clone(),
                    event_type: event_type.to_owned(),
                };
            }

            return WebhookVerdict::Ignored;
        }

        WebhookVerdict::Unknown
    }
}

/// Computes HMAC-SHA256(key, data) using the standard two-pass construction.
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;

    // If key is longer than block size, hash it first.
    let key = if key.len() > BLOCK_SIZE {
        let mut h = Sha256::new();
        h.update(key);
        h.finalize().to_vec()
    } else {
        key.to_vec()
    };

    // Pad key to block size.
    let mut padded = [0u8; BLOCK_SIZE];
    padded[..key.len()].copy_from_slice(&key);

    // Inner hash: H((key XOR ipad) || data)
    let mut inner = Sha256::new();
    let ipad: Vec<u8> = padded.iter().map(|b| b ^ 0x36).collect();
    inner.update(&ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    // Outer hash: H((key XOR opad) || inner_hash)
    let mut outer = Sha256::new();
    let opad: Vec<u8> = padded.iter().map(|b| b ^ 0x5c).collect();
    outer.update(&opad);
    outer.update(inner_hash);
    let result = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Hex-encodes a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Constant-time byte comparison to prevent timing attacks on signatures.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn github_source() -> WebhookSource {
        WebhookSource::github(b"secret123".to_vec(), vec!["push".to_owned()])
    }

    fn sign(secret: &[u8], body: &[u8]) -> String {
        let mac = hmac_sha256(secret, body);
        format!("sha256={}", hex_encode(&mac))
    }

    #[test]
    fn valid_signature_and_matching_trigger_returns_triggered() {
        let listener = EventListener::new(vec![github_source()]);
        let body = b"payload";
        let sig = sign(b"secret123", body);
        let headers = [("x-hub-signature-256", sig.as_str())];

        let verdict = listener.process(&headers, body, "push");
        assert_eq!(
            verdict,
            WebhookVerdict::Triggered {
                source: "github".to_owned(),
                event_type: "push".to_owned(),
            }
        );
    }

    #[test]
    fn valid_signature_non_matching_trigger_returns_ignored() {
        let listener = EventListener::new(vec![github_source()]);
        let body = b"payload";
        let sig = sign(b"secret123", body);
        let headers = [("x-hub-signature-256", sig.as_str())];

        let verdict = listener.process(&headers, body, "star");
        assert_eq!(verdict, WebhookVerdict::Ignored);
    }

    #[test]
    fn invalid_signature_returns_rejected() {
        let listener = EventListener::new(vec![github_source()]);
        let body = b"payload";
        let headers = [("x-hub-signature-256", "sha256=deadbeef")];

        let verdict = listener.process(&headers, body, "push");
        match verdict {
            WebhookVerdict::Rejected { source, reason } => {
                assert_eq!(source, "github");
                assert!(reason.contains("mismatch"));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn no_matching_source_returns_unknown() {
        let listener = EventListener::new(vec![github_source()]);
        let headers = [("x-some-other-header", "value")];
        let verdict = listener.process(&headers, b"body", "push");
        assert_eq!(verdict, WebhookVerdict::Unknown);
    }

    #[test]
    fn empty_triggers_matches_all_events() {
        let mut src = github_source();
        src.triggers.clear();
        let listener = EventListener::new(vec![src]);
        let body = b"data";
        let sig = sign(b"secret123", body);
        let headers = [("x-hub-signature-256", sig.as_str())];

        let verdict = listener.process(&headers, body, "anything");
        assert!(matches!(verdict, WebhookVerdict::Triggered { .. }));
    }

    #[test]
    fn hmac_sha256_known_vector() {
        // RFC 4231 test vector 2.
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let expected = "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
        let result = hmac_sha256(key, data);
        assert_eq!(hex_encode(&result), expected);
    }
}
