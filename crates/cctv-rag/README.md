# cctv-rag

[![Crates.io](https://img.shields.io/crates/v/cctv-rag.svg)](https://crates.io/crates/cctv-rag)
[![docs.rs](https://docs.rs/cctv-rag/badge.svg)](https://docs.rs/cctv-rag)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Deterministic retrieval and diagnostics layer for `cctv-rs`: a temporal incident ledger, a spatial topology graph, and embedded diagnostic playbooks.

Runtime-free by design: no async runtime, no external vector database, and no I/O. Telemetry coalesces into bounded incident windows, cameras and zones link into a queryable graph, and BM25 retrieval returns a prompt-ready context envelope for a diagnosis.

## Primitives

- **`IncidentCoalescer` / `IncidentChunk`**: bounded temporal ledger that coalesces repeated telemetry into graded incident windows (`IncidentSeverity`, `IncidentTelemetry`).
- **`SpatialGraph`**: topology of `CameraNode`, `ZoneNode`, and `PresetNode` joined by `CoversEdge`, `AdjacentToEdge`, and `PtzReachesEdge`, with `HandoffCandidate` selection for PTZ handoffs.
- **`Bm25Index` / `cosine_similarity`**: pure in-memory BM25 scoring over `SearchDocument`s, with optional vector similarity through an `EmbeddingProvider`.
- **`PlaybookRegistry` / `DiagnosticPlaybook`**: the embedded diagnostic knowledge base mapping a retrieved incident to ordered remediation steps.
- **`KnowledgeBaseIndex` / `RagContextEnvelope`**: the unifying entry point that queries incidents, topology, and playbooks together and formats a prompt-ready context block.

## Invariants

- `#![forbid(unsafe_code)]`
- Deterministic: identical inputs and insertion order produce identical retrieval results.
- Embeddable: no async runtime, no sockets, no external database, and bounded memory via the `KnowledgeBaseConfig` eviction limits.

## Usage

```rust
use cctv_rag::KnowledgeBaseIndex;

let index = KnowledgeBaseIndex::new();
let envelope = index
    .query("camera dropped frames after a firmware upgrade", 3)
    .expect("query succeeds");

println!("{}", envelope.formatted_prompt);
```
