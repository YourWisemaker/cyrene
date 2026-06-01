use cyrene_core::{Recoverability, Recoverable};

#[derive(Debug, thiserror::Error)]
pub enum HardwareError {
    #[error("peripheral not found: {0}")]
    NotFound(String),

    #[error("peripheral communication error: {0}")]
    Communication(String),

    #[error("permission denied for peripheral: {0}")]
    PermissionDenied(String),

    #[error("hardware feature not enabled: {0}")]
    FeatureDisabled(String),

    #[error("peripheral I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl Recoverable for HardwareError {
    fn recoverability(&self) -> Recoverability {
        match self {
            Self::NotFound(_) | Self::FeatureDisabled(_) => Recoverability::Halt,
            Self::Communication(_) => Recoverability::Retry,
            Self::PermissionDenied(_) => Recoverability::UserAction,
            Self::Io(_) => Recoverability::Retry,
        }
    }
}
