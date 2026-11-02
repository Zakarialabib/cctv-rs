//! Temporal incident ledger, spatial topology graph, and diagnostic playbooks for `cctv-rs`.
//!
//! Provides deterministic temporal event coalescing, spatial awareness, PTZ handoffs,
//! and embedded diagnostic knowledge retrieval without requiring async runtimes or external databases.

#![forbid(unsafe_code)]

pub mod error;
pub mod incident;
pub mod index;
pub mod playbook;
pub mod retriever;
pub mod topology;

pub use error::RagError;
pub use incident::{IncidentChunk, IncidentCoalescer, IncidentSeverity, IncidentTelemetry};
pub use index::{KnowledgeBaseConfig, KnowledgeBaseIndex};
pub use playbook::{DiagnosticPlaybook, PlaybookRegistry};
pub use retriever::{
    cosine_similarity, tokenize, Bm25Index, EmbeddingProvider, RagContextEnvelope, ScoredResult,
    SearchDocument,
};
pub use topology::{
    AdjacentToEdge, CameraKind, CameraNode, CoversEdge, HandoffCandidate, KnowledgeChunk,
    PresetNode, PtzReachesEdge, SpatialGraph, ZoneKind, ZoneNode,
};
