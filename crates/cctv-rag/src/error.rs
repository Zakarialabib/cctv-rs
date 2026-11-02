//! Error types for RAG, incident tracking, and spatial queries.

use thiserror::Error;

/// Errors arising from the RAG subsystem.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RagError {
    /// An entity (camera, zone, or preset) was not found.
    #[error("entity not found: {0}")]
    NotFound(String),

    /// A capacity limit was exceeded or buffer allocation rejected.
    #[error("capacity exceeded: {0}")]
    CapacityExceeded(String),

    /// Embedding computation or vector generation failed.
    #[error("embedding error: {0}")]
    EmbeddingFailed(String),

    /// Query or configuration was invalid.
    #[error("invalid query or parameter: {0}")]
    InvalidInput(String),
}
