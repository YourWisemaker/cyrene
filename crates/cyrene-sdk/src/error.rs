use thiserror::Error;

/// Errors from the Extension SDK host API.
#[derive(Debug, Error)]
pub enum SdkError {
    #[error("manifest parse error: {0}")]
    ManifestParse(String),

    #[error("manifest validation error: {0}")]
    ManifestValidation(String),

    #[error("host incompatible: extension requires {required}, host is {host}")]
    HostIncompatible { required: String, host: String },

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("ledger error: {0}")]
    Ledger(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("secret not found: {0}")]
    SecretNotFound(String),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
