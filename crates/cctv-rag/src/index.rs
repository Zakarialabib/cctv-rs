//! In-memory bounded knowledge base index and RAG orchestration engine.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::RagError;
use crate::incident::{IncidentChunk, IncidentCoalescer};
use crate::playbook::PlaybookRegistry;
use crate::retriever::{
    cosine_similarity, tokenize, Bm25Index, EmbeddingProvider, RagContextEnvelope, SearchDocument,
};
use crate::topology::SpatialGraph;

/// Configuration for the in-memory knowledge base index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBaseConfig {
    /// Maximum number of temporal incidents retained in memory.
    pub max_incidents: usize,
    /// Coalescing window duration in milliseconds.
    pub window_ms: u64,
    /// Maximum concurrent tracking windows in coalescer.
    pub max_open_windows: usize,
}

impl Default for KnowledgeBaseConfig {
    fn default() -> Self {
        Self {
            max_incidents: 1_000,
            window_ms: 60_000,
            max_open_windows: 200,
        }
    }
}

/// Unified in-memory Knowledge Base and RAG query engine for `cctv-rs`.
pub struct KnowledgeBaseIndex {
    config: KnowledgeBaseConfig,
    coalescer: IncidentCoalescer,
    incidents: BTreeMap<String, IncidentChunk>,
    topology: SpatialGraph,
    playbooks: PlaybookRegistry,
    embedding_provider: Option<Box<dyn EmbeddingProvider>>,
}

impl KnowledgeBaseIndex {
    /// Create a new knowledge base with default configuration and standard playbooks.
    pub fn new() -> Self {
        Self::with_config(KnowledgeBaseConfig::default())
    }

    /// Create with specific configuration.
    pub fn with_config(config: KnowledgeBaseConfig) -> Self {
        let coalescer = IncidentCoalescer::new(config.window_ms, config.max_open_windows);
        Self {
            config,
            coalescer,
            incidents: BTreeMap::new(),
            topology: SpatialGraph::new(),
            playbooks: PlaybookRegistry::new(),
            embedding_provider: None,
        }
    }

    /// Set an optional embedding provider for hybrid vector similarity search.
    pub fn with_embedding_provider(mut self, provider: Box<dyn EmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }

    /// Access the internal incident coalescer mutably.
    pub fn coalescer_mut(&mut self) -> &mut IncidentCoalescer {
        &mut self.coalescer
    }

    /// Access the spatial topology graph mutably.
    pub fn topology_mut(&mut self) -> &mut SpatialGraph {
        &mut self.topology
    }

    /// Access the spatial topology graph.
    pub fn topology(&self) -> &SpatialGraph {
        &self.topology
    }

    /// Access the playbook registry mutably.
    pub fn playbooks_mut(&mut self) -> &mut PlaybookRegistry {
        &mut self.playbooks
    }

    /// Access the playbook registry.
    pub fn playbooks(&self) -> &PlaybookRegistry {
        &self.playbooks
    }

    /// Insert an incident chunk into the ledger, enforcing bounded memory eviction.
    pub fn insert_incident(&mut self, chunk: IncidentChunk) {
        if self.incidents.len() >= self.config.max_incidents {
            // Evict oldest incident chunk by time_window.0
            let oldest_id = self
                .incidents
                .iter()
                .min_by_key(|(_, chunk)| chunk.time_window.0)
                .map(|(id, _)| id.clone());

            if let Some(id) = oldest_id {
                self.incidents.remove(&id);
            }
        }
        self.incidents.insert(chunk.chunk_id.clone(), chunk);
    }

    /// Tick the coalescer with current timestamp, synthesize expired windows, and insert into ledger.
    pub fn tick(&mut self, now_ms: u64) -> Vec<IncidentChunk> {
        let chunks = self.coalescer.flush_expired(now_ms);
        for chunk in &chunks {
            self.insert_incident(chunk.clone());
        }
        chunks
    }

    /// Query the knowledge base and produce a structured [`RagContextEnvelope`].
    pub fn query(&self, query_text: &str, top_k: usize) -> Result<RagContextEnvelope, RagError> {
        let mut docs = Vec::new();

        // 1. Incidents
        for incident in self.incidents.values() {
            docs.push(SearchDocument {
                id: incident.chunk_id.clone(),
                doc_type: "incident".to_string(),
                entity_id: Some(incident.camera_id.to_string()),
                text: incident.narrative.clone(),
                tags: incident.tags.clone(),
                embedding: None,
            });
        }

        // 2. Spatial Topology Chunks
        for top_chunk in self.topology.to_knowledge_chunks() {
            docs.push(SearchDocument {
                id: top_chunk.chunk_id,
                doc_type: "topology".to_string(),
                entity_id: Some(top_chunk.entity_id),
                text: top_chunk.narrative,
                tags: top_chunk.tags,
                embedding: None,
            });
        }

        // 3. Diagnostic Playbooks
        for playbook in self.playbooks.all() {
            docs.push(SearchDocument {
                id: playbook.playbook_id.clone(),
                doc_type: "playbook".to_string(),
                entity_id: Some(playbook.subsystem.clone()),
                text: playbook.to_narrative(),
                tags: playbook.tags.clone(),
                embedding: None,
            });
        }

        if docs.is_empty() {
            return Ok(RagContextEnvelope::new(query_text.to_string(), Vec::new()));
        }

        // BM25 scoring
        let query_tokens = tokenize(query_text);
        let bm25 = Bm25Index::build(&docs);

        let query_embedding = if let Some(provider) = &self.embedding_provider {
            let emb = provider.embed(query_text)?;
            Some(emb)
        } else {
            None
        };

        let mut scored: Vec<(f32, SearchDocument)> = docs
            .into_iter()
            .map(|doc| {
                let bm25_score = bm25.score(&doc.id, &query_tokens);
                let vector_score =
                    if let (Some(q_emb), Some(d_emb)) = (&query_embedding, &doc.embedding) {
                        cosine_similarity(q_emb, d_emb)
                    } else {
                        0.0
                    };
                let combined_score = bm25_score + (vector_score * 2.0);
                (combined_score, doc)
            })
            .collect();

        // Sort descending by score
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let top_docs: Vec<SearchDocument> = scored
            .into_iter()
            .filter(|(s, _)| *s > 0.0)
            .take(top_k.max(1))
            .map(|(_, doc)| doc)
            .collect();

        Ok(RagContextEnvelope::new(query_text.to_string(), top_docs))
    }
}

impl Default for KnowledgeBaseIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incident::IncidentSeverity;
    use cctv_core::CameraId;

    #[test]
    fn test_rag_query_retrieval_and_envelope() {
        let mut index = KnowledgeBaseIndex::new();

        // Add incident
        index.insert_incident(IncidentChunk {
            chunk_id: "cam-warehouse:1000-2000".to_string(),
            time_window: (1000, 2000),
            camera_id: CameraId::new("cam-warehouse"),
            zone_id: Some("dock".to_string()),
            severity: IncidentSeverity::Degraded,
            tags: vec!["rtsp_401".to_string(), "clock_skew".to_string()],
            narrative: "Camera cam-warehouse dropped frames due to RTSP 401 and WS-Security clock skew failure.".to_string(),
            telemetry: Default::default(),
        });

        let envelope = index
            .query("Why did cam-warehouse experience clock skew?", 3)
            .expect("query success");
        assert_eq!(envelope.chunks.len(), 2); // Matches incident and clock_skew playbook!
        assert!(envelope.formatted_prompt.contains("cam-warehouse"));
        assert!(envelope.formatted_prompt.contains("ClockHealing"));
    }
}
