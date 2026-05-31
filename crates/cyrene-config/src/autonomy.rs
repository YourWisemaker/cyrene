//! Autonomy and security configuration with secure-by-default values (R22).
//!
//! The defaults encode Requirement 22.1: medium-risk actions require approval
//! and high-risk actions are blocked. Raising autonomy is therefore an
//! *explicit* edit to the config file (R22.4) — the deserialized defaults never
//! grant more than the safe baseline. Each field uses serde `default`
//! attributes so an omitted `[autonomy]` section still yields the secure
//! baseline rather than failing to load.

use cyrene_core::Risk;
use serde::{Deserialize, Serialize};

/// How Cyrene treats a step of a given [`Risk`] under the autonomy policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyAction {
    /// Execute automatically without user involvement.
    Auto,
    /// Halt and request user approval before executing (Approval_Gate, R6).
    Approval,
    /// Refuse to execute until autonomy is explicitly raised.
    Blocked,
}

/// The autonomy and security policy section of the config file (R22).
///
/// Defaults are deliberately conservative: `low = auto`, `medium = approval`,
/// `high = blocked` (R22.1). Because every field defaults to the safe value,
/// a config that omits `[autonomy]` entirely still loads as secure-by-default,
/// and loosening the policy requires an explicit, reviewable config change
/// (R22.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutonomyConfig {
    /// Action applied to low-risk steps. Defaults to [`AutonomyAction::Auto`].
    pub low: AutonomyAction,
    /// Action applied to medium-risk steps. Defaults to
    /// [`AutonomyAction::Approval`] (R22.1).
    pub medium: AutonomyAction,
    /// Action applied to high-risk steps. Defaults to
    /// [`AutonomyAction::Blocked`] (R22.1).
    pub high: AutonomyAction,
    /// Commands permitted to run without per-step approval (R22.2). Empty by
    /// default: every command requires approval until the user opts specific
    /// ones in.
    pub command_allowlist: Vec<String>,
    /// Whether a network-exposed gateway requires authentication (R22.5).
    /// Defaults to `true`; it should only be disabled in a trusted, isolated
    /// environment and doing so is an explicit choice.
    pub require_gateway_auth: bool,
}

impl Default for AutonomyConfig {
    /// The secure-by-default policy mandated by R22.1.
    fn default() -> Self {
        Self {
            low: AutonomyAction::Auto,
            medium: AutonomyAction::Approval,
            high: AutonomyAction::Blocked,
            command_allowlist: Vec::new(),
            require_gateway_auth: true,
        }
    }
}

impl AutonomyConfig {
    /// Returns the configured [`AutonomyAction`] for a step of the given
    /// [`Risk`].
    #[must_use]
    pub fn action_for(&self, risk: Risk) -> AutonomyAction {
        match risk {
            Risk::Low => self.low,
            Risk::Medium => self.medium,
            Risk::High => self.high,
        }
    }

    /// Returns `true` if `command` is on the configured allowlist (R22.2).
    ///
    /// Matching is on the leading whitespace-delimited token (the program
    /// name), so an allowlisted `git` permits `git status` but not a different
    /// program. An empty allowlist permits nothing.
    #[must_use]
    pub fn is_command_allowed(&self, command: &str) -> bool {
        let program = command.split_whitespace().next().unwrap_or("");
        if program.is_empty() {
            return false;
        }
        self.command_allowlist
            .iter()
            .any(|allowed| allowed == program)
    }

    /// Returns `true` if this policy is at least as strict as the secure
    /// default for medium and high risk (medium not auto, high not auto).
    ///
    /// Useful for surfacing a warning when a user has loosened the baseline.
    #[must_use]
    pub fn is_secure_default_or_stricter(&self) -> bool {
        self.medium != AutonomyAction::Auto && self.high != AutonomyAction::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::{AutonomyAction, AutonomyConfig};
    use cyrene_core::Risk;

    #[test]
    fn default_is_secure_by_default() {
        let cfg = AutonomyConfig::default();
        assert_eq!(cfg.action_for(Risk::Low), AutonomyAction::Auto);
        assert_eq!(cfg.action_for(Risk::Medium), AutonomyAction::Approval);
        assert_eq!(cfg.action_for(Risk::High), AutonomyAction::Blocked);
        assert!(cfg.require_gateway_auth);
        assert!(cfg.command_allowlist.is_empty());
        assert!(cfg.is_secure_default_or_stricter());
    }

    #[test]
    fn omitted_section_deserializes_to_secure_default() {
        // An empty table must yield the safe baseline (serde `default`).
        let cfg: AutonomyConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, AutonomyConfig::default());
    }

    #[test]
    fn raising_autonomy_requires_explicit_config() {
        // Loosening medium to auto only happens when explicitly written.
        let cfg: AutonomyConfig = toml::from_str("medium = \"auto\"").unwrap();
        assert_eq!(cfg.action_for(Risk::Medium), AutonomyAction::Auto);
        // The rest stay at the secure default.
        assert_eq!(cfg.action_for(Risk::High), AutonomyAction::Blocked);
        assert!(!cfg.is_secure_default_or_stricter());
    }

    #[test]
    fn command_allowlist_matches_program_token() {
        let cfg: AutonomyConfig = toml::from_str("command_allowlist = [\"git\", \"ls\"]").unwrap();
        assert!(cfg.is_command_allowed("git status"));
        assert!(cfg.is_command_allowed("ls -la"));
        assert!(!cfg.is_command_allowed("rm -rf /"));
        assert!(!cfg.is_command_allowed(""));
    }

    #[test]
    fn unknown_field_is_rejected() {
        // `deny_unknown_fields` guards against typos silently loosening policy.
        assert!(toml::from_str::<AutonomyConfig>("blocked = \"auto\"").is_err());
    }

    #[test]
    fn action_round_trips_snake_case() {
        for action in [
            AutonomyAction::Auto,
            AutonomyAction::Approval,
            AutonomyAction::Blocked,
        ] {
            let s = toml::to_string(&Wrap { a: action }).unwrap();
            let back: Wrap = toml::from_str(&s).unwrap();
            assert_eq!(back.a, action);
        }
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Wrap {
        a: AutonomyAction,
    }
}
