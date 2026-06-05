//! Error model for `cyrene-config`.
//!
//! Follows the design's "Error Handling" convention: a `thiserror` enum per
//! crate that carries a [`Recoverability`] hint so the Agent_Loop can decide
//! how to react. Configuration failures are either unrecoverable (`Halt`,
//! e.g. malformed TOML) or need the user to act (`UserAction`, e.g. an unset
//! secret environment variable or a missing required section).

use std::path::PathBuf;

use cyrene_core::{Recoverability, Recoverable};

/// Errors raised while loading or validating Cyrene configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The configuration file could not be read from disk.
    #[error("failed to read config file `{path}`: {source}")]
    Io {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// The configuration file contained malformed TOML.
    #[error("failed to parse config file `{path}`: {source}")]
    Parse {
        /// The path that failed to parse.
        path: PathBuf,
        /// The underlying TOML deserialization error.
        source: toml::de::Error,
    },

    /// A required configuration section was absent.
    ///
    /// A single config file must declare at least one Model_Provider and one
    /// Channel for the runtime to do anything useful (R2.5).
    #[error("config is missing a required `[{0}]` section")]
    MissingSection(&'static str),

    /// A referenced secret was not present in the environment / `.env` file.
    ///
    /// Secrets are never stored in the TOML; the config references them by
    /// environment-variable name only (R22, secret hygiene).
    #[error(
        "secret environment variable `{0}` is not set (define it in your environment or `.env`)"
    )]
    MissingSecret(String),

    /// The selected remote execution backend is missing required settings.
    ///
    /// Selecting an SSH or container-host backend (R33.5) requires its
    /// `[execution.<backend>]` section with a host/image and a remote workspace
    /// boundary so the autonomy/sandbox/approval pipeline still has a boundary
    /// to enforce against.
    #[error("invalid `[execution]` configuration: {0}")]
    InvalidExecution(String),

    /// The default config path could not be resolved because the user's home
    /// directory is unknown.
    #[error("could not determine the home directory for the default config path (~/.cyrene/config.toml)")]
    NoHomeDir,
}

impl Recoverable for ConfigError {
    fn recoverability(&self) -> Recoverability {
        match self {
            // The user must create/edit config, set a secret, or fix their env.
            Self::MissingSection(_)
            | Self::MissingSecret(_)
            | Self::InvalidExecution(_)
            | Self::NoHomeDir => Recoverability::UserAction,
            // A malformed or unreadable file cannot be recovered automatically.
            Self::Io { .. } | Self::Parse { .. } => Recoverability::Halt,
        }
    }
}
