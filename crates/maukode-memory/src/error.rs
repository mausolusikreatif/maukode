use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("memory not found: {0}")]
    NotFound(String),

    #[error("version conflict: expected {expected}, found {actual}")]
    VersionConflict { expected: i64, actual: i64 },

    #[error("title already exists in this scope: {0}")]
    DuplicateTitle(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, MemoryError>;
