//! Secret resolution from the environment and an optional `.env` file.
//!
//! Secrets (API keys, channel tokens) are **never** stored in the TOML config
//! file. The TOML may only reference a secret by the *name* of the environment
//! variable that holds it; the [`SecretResolver`] reads the actual value from
//! the process environment, optionally seeded from a `.env` file via
//! [`dotenvy`]. This keeps every committed file free of real secrets
//! (R22, secret hygiene) and lets the same config work across machines and
//! deployments where only the environment differs.

use std::path::Path;

use crate::error::ConfigError;

/// Resolves secret values by environment-variable name.
///
/// Construct it with [`SecretResolver::from_env`] (process environment only) or
/// [`SecretResolver::with_dotenv`] / [`SecretResolver::with_dotenv_path`] to
/// first load a `.env` file into the environment. The resolver itself holds no
/// secret material; it reads from [`std::env`] on demand so values are never
/// copied into long-lived config structs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SecretResolver {
    _private: (),
}

impl SecretResolver {
    /// Creates a resolver backed by the current process environment.
    ///
    /// Does not load any `.env` file; use [`SecretResolver::with_dotenv`] to do
    /// that first.
    #[must_use]
    pub fn from_env() -> Self {
        Self { _private: () }
    }

    /// Loads a `.env` file from the default location (the nearest `.env`
    /// walking up from the current directory), then returns a resolver.
    ///
    /// A missing `.env` file is **not** an error: secrets may come purely from
    /// the real environment (e.g. in production / containers, R33.3). Existing
    /// environment variables are never overwritten by the file.
    #[must_use]
    pub fn with_dotenv() -> Self {
        // Ignore a missing file; only a present-but-unreadable file would error,
        // and we still prefer to fall back to the live environment.
        let _ = dotenvy::dotenv();
        Self::from_env()
    }

    /// Loads a `.env` file from an explicit path, then returns a resolver.
    ///
    /// A missing file at `path` is ignored (the environment may already hold
    /// the secrets). Existing environment variables take precedence over the
    /// file's values.
    pub fn with_dotenv_path(path: impl AsRef<Path>) -> Self {
        let _ = dotenvy::from_path(path.as_ref());
        Self::from_env()
    }

    /// Returns the secret value stored in the environment variable `key`.
    ///
    /// # Errors
    /// Returns [`ConfigError::MissingSecret`] if the variable is unset or empty.
    pub fn require(&self, key: &str) -> Result<String, ConfigError> {
        match self.get(key) {
            Some(value) => Ok(value),
            None => Err(ConfigError::MissingSecret(key.to_owned())),
        }
    }

    /// Returns the secret value for `key`, or [`None`] if it is unset or empty.
    ///
    /// An empty value is treated as absent so a placeholder line like
    /// `OPENAI_API_KEY=` in a `.env` does not read as a real secret.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
        match std::env::var(key) {
            Ok(value) if !value.is_empty() => Some(value),
            _ => None,
        }
    }

    /// Returns `true` if a non-empty secret is present for `key`.
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::SecretResolver;
    use crate::error::ConfigError;

    // Tests mutate process-global environment variables, so each uses a unique
    // key to avoid cross-test interference under parallel execution.

    #[test]
    fn require_returns_value_when_set() {
        let key = "CYRENE_TEST_SECRET_REQUIRE";
        std::env::set_var(key, "sk-fake-value");
        let resolver = SecretResolver::from_env();
        assert_eq!(resolver.require(key).unwrap(), "sk-fake-value");
        std::env::remove_var(key);
    }

    #[test]
    fn require_errors_when_unset() {
        let key = "CYRENE_TEST_SECRET_UNSET";
        std::env::remove_var(key);
        let resolver = SecretResolver::from_env();
        match resolver.require(key) {
            Err(ConfigError::MissingSecret(missing)) => assert_eq!(missing, key),
            other => panic!("expected MissingSecret, got {other:?}"),
        }
    }

    #[test]
    fn empty_value_is_treated_as_absent() {
        let key = "CYRENE_TEST_SECRET_EMPTY";
        std::env::set_var(key, "");
        let resolver = SecretResolver::from_env();
        assert!(!resolver.has(key));
        assert!(resolver.get(key).is_none());
        assert!(matches!(
            resolver.require(key),
            Err(ConfigError::MissingSecret(_))
        ));
        std::env::remove_var(key);
    }

    #[test]
    fn has_and_get_reflect_presence() {
        let key = "CYRENE_TEST_SECRET_HAS";
        std::env::remove_var(key);
        let resolver = SecretResolver::from_env();
        assert!(!resolver.has(key));
        std::env::set_var(key, "token");
        assert!(resolver.has(key));
        assert_eq!(resolver.get(key).as_deref(), Some("token"));
        std::env::remove_var(key);
    }
}
