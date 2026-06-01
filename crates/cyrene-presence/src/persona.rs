//! The Persona_Engine: adaptive communication tone (R18).
//!
//! Cyrene adapts its tone, verbosity, and urgency to the context:
//!
//! - **Emergency** (R18.1): short, prioritized messages; no pleasantries.
//! - **Creative** (R18.2): relaxed, conversational style.
//! - **Away** (R18.3): contextual wrap-up summarizing what was completed.
//!
//! A persona can be configured per channel or per session (R18.4). The engine
//! selects the active persona and shapes the response text accordingly.

use serde::{Deserialize, Serialize};

/// The communication persona applied to a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Persona {
    /// Default balanced tone.
    Default,
    /// Emergency: crisp, prioritized, no pleasantries (R18.1).
    Emergency,
    /// Creative/exploratory: relaxed, conversational (R18.2).
    Creative,
    /// Away/wrap-up: summarize what was done, ready for review (R18.3).
    Away,
}

impl Default for Persona {
    fn default() -> Self {
        Self::Default
    }
}

/// The context used to select a persona.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaContext {
    /// The channel the response will be delivered on.
    pub channel: String,
    /// An optional session-level persona override.
    pub session_persona: Option<Persona>,
    /// An optional channel-level persona override.
    pub channel_persona: Option<Persona>,
    /// Whether the user is currently away (triggers Away persona, R18.3).
    pub user_away: bool,
    /// Whether the context is classified as an emergency.
    pub is_emergency: bool,
    /// Whether the context is classified as creative/exploratory.
    pub is_creative: bool,
}

impl PersonaContext {
    /// Creates a default context for a channel.
    #[must_use]
    pub fn new(channel: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            session_persona: None,
            channel_persona: None,
            user_away: false,
            is_emergency: false,
            is_creative: false,
        }
    }
}

/// The Persona_Engine: selects and applies a persona to shape responses.
#[derive(Debug, Clone, Default)]
pub struct PersonaEngine;

impl PersonaEngine {
    /// Creates a new persona engine.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Selects the active persona for the given context (R18.4).
    ///
    /// Priority: session override > channel override > context signals > default.
    #[must_use]
    pub fn select(&self, ctx: &PersonaContext) -> Persona {
        // Explicit overrides take precedence (R18.4).
        if let Some(p) = ctx.session_persona {
            return p;
        }
        if let Some(p) = ctx.channel_persona {
            return p;
        }
        // Context signals.
        if ctx.is_emergency {
            return Persona::Emergency;
        }
        if ctx.user_away {
            return Persona::Away;
        }
        if ctx.is_creative {
            return Persona::Creative;
        }
        Persona::Default
    }

    /// Shapes a response text according to the active persona.
    ///
    /// This is a lightweight shaping pass — the model itself is prompted with
    /// the persona, but this post-processing ensures structural compliance
    /// (e.g. stripping pleasantries in emergency mode).
    #[must_use]
    pub fn shape(&self, text: &str, persona: Persona) -> String {
        match persona {
            Persona::Emergency => {
                // Strip common pleasantries and keep it short (R18.1).
                let stripped = text
                    .lines()
                    .filter(|l| {
                        let lower = l.to_lowercase();
                        !lower.starts_with("hi ")
                            && !lower.starts_with("hello")
                            && !lower.starts_with("hey ")
                            && !lower.starts_with("sure,")
                            && !lower.starts_with("of course")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                stripped.trim().to_owned()
            }
            Persona::Away => {
                // Prepend a wrap-up header (R18.3).
                format!("While you were away:\n{text}")
            }
            Persona::Creative | Persona::Default => {
                // No structural change; the model handles tone via prompting.
                text.to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_selects_default_persona() {
        let engine = PersonaEngine::new();
        let ctx = PersonaContext::new("cli");
        assert_eq!(engine.select(&ctx), Persona::Default);
    }

    #[test]
    fn emergency_context_selects_emergency() {
        let engine = PersonaEngine::new();
        let mut ctx = PersonaContext::new("telegram");
        ctx.is_emergency = true;
        assert_eq!(engine.select(&ctx), Persona::Emergency);
    }

    #[test]
    fn creative_context_selects_creative() {
        let engine = PersonaEngine::new();
        let mut ctx = PersonaContext::new("slack");
        ctx.is_creative = true;
        assert_eq!(engine.select(&ctx), Persona::Creative);
    }

    #[test]
    fn away_user_selects_away() {
        let engine = PersonaEngine::new();
        let mut ctx = PersonaContext::new("cli");
        ctx.user_away = true;
        assert_eq!(engine.select(&ctx), Persona::Away);
    }

    #[test]
    fn session_override_takes_precedence() {
        let engine = PersonaEngine::new();
        let mut ctx = PersonaContext::new("cli");
        ctx.is_emergency = true;
        ctx.session_persona = Some(Persona::Creative);
        assert_eq!(engine.select(&ctx), Persona::Creative);
    }

    #[test]
    fn channel_override_takes_precedence_over_signals() {
        let engine = PersonaEngine::new();
        let mut ctx = PersonaContext::new("cli");
        ctx.is_creative = true;
        ctx.channel_persona = Some(Persona::Emergency);
        assert_eq!(engine.select(&ctx), Persona::Emergency);
    }

    #[test]
    fn emergency_shape_strips_pleasantries() {
        let engine = PersonaEngine::new();
        let text = "Hello! Sure, here's the fix:\nApply patch X.";
        let shaped = engine.shape(text, Persona::Emergency);
        assert!(!shaped.contains("Hello"));
        assert!(!shaped.contains("Sure,"));
        assert!(shaped.contains("Apply patch X."));
    }

    #[test]
    fn away_shape_prepends_wrap_up_header() {
        let engine = PersonaEngine::new();
        let text = "Deployed v2.1.0 to production.";
        let shaped = engine.shape(text, Persona::Away);
        assert!(shaped.starts_with("While you were away:"));
        assert!(shaped.contains("Deployed v2.1.0"));
    }

    #[test]
    fn default_shape_passes_through() {
        let engine = PersonaEngine::new();
        let text = "Here's the result.";
        assert_eq!(engine.shape(text, Persona::Default), text);
    }

    #[test]
    fn persona_serde_round_trip() {
        for p in [
            Persona::Default,
            Persona::Emergency,
            Persona::Creative,
            Persona::Away,
        ] {
            let json = serde_json::to_string(&p).unwrap();
            let back: Persona = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }
    }
}
