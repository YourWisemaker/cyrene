//! Error model for the Skill_Engine and Skill_Library.

/// Errors the Skill_Engine and Skill_Library can return.
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    /// A `SKILL.md` definition could not be parsed.
    #[error("invalid SKILL.md: {0}")]
    Parse(String),

    /// The candidate skill failed its mandatory sandbox test (R14.3).
    #[error("candidate skill failed sandbox test: {0}")]
    SandboxFailed(String),

    /// A skill with the requested id was not found in the library.
    #[error("skill not found: {0}")]
    NotFound(String),

    /// The underlying storage (filesystem) failed.
    #[error("skill storage error: {0}")]
    Storage(String),

    /// A skill definition failed to (de)serialize.
    #[error("skill serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl From<std::io::Error> for SkillError {
    fn from(e: std::io::Error) -> Self {
        Self::Storage(e.to_string())
    }
}
