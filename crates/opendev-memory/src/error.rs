//! Unified error types for the memory crate.

/// Errors that can occur during memory system operations.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// Embedding computation or retrieval failed.
    #[error("embedding failed: {0}")]
    EmbeddingFailed(String),

    /// Cache operation failed.
    #[error("cache error: {0}")]
    CacheError(String),

    /// JSON serialization/deserialization error.
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// I/O error (e.g., file read/write).
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid input (e.g., unsupported tag name).
    #[error("invalid input: {0}")]
    InvalidInput(String),
}
