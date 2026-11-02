//! Retrieval abstractions, BM25 keyword scoring, and RAG context envelope.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

use crate::error::RagError;

/// Pure trait for generating text embeddings (e.g., via ONNX, Candle, or cloud AI).
pub trait EmbeddingProvider: Send + Sync {
    /// Compute an embedding vector for the given text.
    fn embed(&self, text: &str) -> Result<Vec<f32>, RagError>;
}

/// Tokenize and normalize text for in-memory keyword / BM25 search.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

/// Compute cosine similarity between two normalized or raw vectors.
pub fn cosine_similarity(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.len() != v2.len() || v1.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut norm1 = 0.0;
    let mut norm2 = 0.0;
    for (a, b) in v1.iter().zip(v2.iter()) {
        dot += a * b;
        norm1 += a * a;
        norm2 += b * b;
    }
    let denom = norm1.sqrt() * norm2.sqrt();
    if denom <= f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

/// Document representation for retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchDocument {
    /// Identifier.
    pub id: String,
    /// Categorical type: "incident", "topology", or "playbook".
    pub doc_type: String,
    /// Associated entity (e.g. camera id, zone id, or subsystem).
    pub entity_id: Option<String>,
    /// Natural language text.
    pub text: String,
    /// Keyword tags.
    pub tags: Vec<String>,
    /// Optional pre-computed embedding vector.
    pub embedding: Option<Vec<f32>>,
}

/// Scored search result.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredResult<'a> {
    /// The document matched.
    pub doc: &'a SearchDocument,
    /// Relevance score.
    pub score: f32,
}

/// Pure in-memory BM25 index over documents.
#[derive(Debug, Clone, Default)]
pub struct Bm25Index {
    doc_count: usize,
    avg_doc_len: f32,
    doc_lens: BTreeMap<String, usize>,
    term_doc_counts: BTreeMap<String, usize>,
    term_freqs: BTreeMap<String, BTreeMap<String, usize>>, // term -> doc_id -> count
}

impl Bm25Index {
    /// Build index from documents.
    pub fn build(docs: &[SearchDocument]) -> Self {
        let mut doc_lens = BTreeMap::new();
        let mut term_doc_counts = BTreeMap::new();
        let mut term_freqs = BTreeMap::new();
        let mut total_len = 0;

        for doc in docs {
            let tokens = tokenize(&doc.text);
            let len = tokens.len();
            total_len += len;
            doc_lens.insert(doc.id.clone(), len);

            let mut unique_terms = HashSet::new();
            for term in tokens {
                unique_terms.insert(term.clone());
                let entry = term_freqs
                    .entry(term)
                    .or_insert_with(BTreeMap::new)
                    .entry(doc.id.clone())
                    .or_insert(0);
                *entry += 1;
            }

            for term in unique_terms {
                *term_doc_counts.entry(term).or_insert(0) += 1;
            }
        }

        let doc_count = docs.len();
        let avg_doc_len = if doc_count > 0 {
            total_len as f32 / doc_count as f32
        } else {
            0.0
        };

        Self {
            doc_count,
            avg_doc_len,
            doc_lens,
            term_doc_counts,
            term_freqs,
        }
    }

    /// Score a document against query terms using standard BM25 formula.
    pub fn score(&self, doc_id: &str, query_terms: &[String]) -> f32 {
        let doc_len = match self.doc_lens.get(doc_id) {
            Some(&len) => len as f32,
            None => return 0.0,
        };

        let k1 = 1.2;
        let b = 0.75;
        let mut total_score = 0.0;

        for term in query_terms {
            let n = match self.term_doc_counts.get(term) {
                Some(&cnt) => cnt as f32,
                None => continue,
            };

            let tf = self
                .term_freqs
                .get(term)
                .and_then(|m| m.get(doc_id))
                .copied()
                .unwrap_or(0) as f32;

            if tf <= 0.0 {
                continue;
            }

            // Standard Robertson-Sparck Jones IDF
            let idf = ((self.doc_count as f32 - n + 0.5) / (n + 0.5) + 1.0).ln();
            let denom = tf + k1 * (1.0 - b + b * (doc_len / self.avg_doc_len.max(1.0)));
            total_score += idf * (tf * (k1 + 1.0) / denom);
        }

        total_score
    }
}

/// A structured RAG context envelope ready to inject into an LLM prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RagContextEnvelope {
    /// Search query that generated this context.
    pub query: String,
    /// Retrieved relevant context chunks.
    pub chunks: Vec<SearchDocument>,
    /// System prompt preamble formatted with incident and diagnostic insights.
    pub formatted_prompt: String,
}

impl RagContextEnvelope {
    /// Format retrieved documents into an LLM context block.
    pub fn new(query: String, chunks: Vec<SearchDocument>) -> Self {
        let mut sections = Vec::new();
        sections.push(format!(
            "### RELEVANT CCTV-RS CONTEXT FOR QUERY: \"{query}\""
        ));

        for (i, chunk) in chunks.iter().enumerate() {
            let header = match chunk.doc_type.as_str() {
                "incident" => format!(
                    "[Context Chunk #{}: Temporal Incident ({})]",
                    i + 1,
                    chunk.id
                ),
                "topology" => format!(
                    "[Context Chunk #{}: Spatial Topology ({})]",
                    i + 1,
                    chunk.id
                ),
                "playbook" => format!(
                    "[Context Chunk #{}: Diagnostic Playbook ({})]",
                    i + 1,
                    chunk.id
                ),
                _ => format!("[Context Chunk #{}: {}]", i + 1, chunk.id),
            };
            sections.push(format!("{header}\n{}", chunk.text));
        }

        sections.push("### INSTRUCTIONS FOR OPERATOR ASSISTANT:".to_string());
        sections.push("1. Synthesize the above incident telemetry, spatial topology, and diagnostic playbooks to answer the query accurately.".to_string());
        sections.push("2. Reference specific camera IDs, zones, and protocol failure mechanisms when diagnosing issues.".to_string());

        let formatted_prompt = sections.join("\n\n");

        Self {
            query,
            chunks,
            formatted_prompt,
        }
    }
}
