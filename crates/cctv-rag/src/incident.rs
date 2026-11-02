//! Sliding-window temporal coalescer and semantic incident synthesizer.
//!
//! Converts high-frequency motion bursts, transport drop metrics, and reconciler
//! audit events into compact, bounded incident chunks suitable for indexing and LLM context.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use cctv_core::{AuditEntry, CameraId, MotionEvent};

/// Severity rating for an incident chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IncidentSeverity {
    /// Nominal operational telemetry or isolated minor motion.
    Info,
    /// Sustained motion bursts or minor recoverable packet/frame loss.
    Warning,
    /// Significant frame loss, GOP starvation, or authentication failures.
    Degraded,
    /// Hardware failure, continuous authentication lockout, or unrecoverable stream drops.
    Critical,
}

/// Telemetry metrics summarized inside an incident.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IncidentTelemetry {
    /// Number of motion events detected in the window.
    pub motion_count: u32,
    /// Peak motion detection confidence observed (0.0..1.0).
    pub peak_confidence: f32,
    /// Number of dropped frames reported by the transport layer.
    pub dropped_frames: u64,
    /// Number of starved GOP intervals observed.
    pub starved_gops: u64,
    /// The most significant remediation or audit action recorded.
    pub remedy_applied: Option<String>,
    /// Estimated time in milliseconds taken to remediate/recover, if known.
    pub recovery_ms: Option<u64>,
}

/// A structured, semantic chunk representing an incident over a bounded time window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IncidentChunk {
    /// Unique deterministic identifier: `"{camera_id}:{start_ms}-{end_ms}"`.
    pub chunk_id: String,
    /// The inclusive-exclusive time window `(start_ms, end_ms)`.
    pub time_window: (u64, u64),
    /// The primary camera associated with this incident.
    pub camera_id: CameraId,
    /// Physical detection zone ID if known.
    pub zone_id: Option<String>,
    /// Incident severity.
    pub severity: IncidentSeverity,
    /// Search and filter tags (e.g., `["motion_burst", "rtsp_401", "gop_starvation"]`).
    pub tags: Vec<String>,
    /// Dense, deterministic natural language narrative for LLM context and BM25 indexing.
    pub narrative: String,
    /// Detailed telemetry snapshot.
    pub telemetry: IncidentTelemetry,
}

/// An open sliding window accumulating telemetry for a camera and optional zone.
#[derive(Debug, Clone)]
struct OpenWindow {
    start_ms: u64,
    last_update_ms: u64,
    camera_id: CameraId,
    zone_id: Option<String>,
    motion_count: u32,
    peak_confidence: f32,
    dropped_frames: u64,
    starved_gops: u64,
    audit_events: Vec<String>,
    tags: Vec<String>,
    severity: IncidentSeverity,
}

impl OpenWindow {
    fn new(camera_id: CameraId, zone_id: Option<String>, start_ms: u64) -> Self {
        Self {
            start_ms,
            last_update_ms: start_ms,
            camera_id,
            zone_id,
            motion_count: 0,
            peak_confidence: 0.0,
            dropped_frames: 0,
            starved_gops: 0,
            audit_events: Vec::new(),
            tags: Vec::new(),
            severity: IncidentSeverity::Info,
        }
    }

    fn add_tag(&mut self, tag: &str) {
        if !self.tags.iter().any(|t| t == tag) {
            self.tags.push(tag.to_string());
        }
    }

    fn synthesize(self, end_ms: u64) -> IncidentChunk {
        let chunk_id = format!("{}:{}-{}", self.camera_id, self.start_ms, end_ms);
        let remedy = self.audit_events.last().cloned();
        let recovery_ms = if remedy.is_some() {
            Some(end_ms.saturating_sub(self.start_ms))
        } else {
            None
        };

        // Construct narrative
        let mut parts = Vec::new();
        let zone_str = self
            .zone_id
            .as_deref()
            .map(|z| format!(" [Zone: {z}]"))
            .unwrap_or_default();

        parts.push(format!(
            "Camera {}{zone_str} incident between {}-{} ms.",
            self.camera_id, self.start_ms, end_ms
        ));

        if self.motion_count > 0 {
            parts.push(format!(
                "Recorded {} motion detections (peak confidence: {:.2}).",
                self.motion_count, self.peak_confidence
            ));
        }

        if self.dropped_frames > 0 || self.starved_gops > 0 {
            parts.push(format!(
                "Transport metrics: {} dropped frames, {} starved GOPs.",
                self.dropped_frames, self.starved_gops
            ));
        }

        if !self.audit_events.is_empty() {
            parts.push(format!(
                "Reconciler events: {}.",
                self.audit_events.join("; ")
            ));
        }

        let narrative = parts.join(" ");

        let telemetry = IncidentTelemetry {
            motion_count: self.motion_count,
            peak_confidence: self.peak_confidence,
            dropped_frames: self.dropped_frames,
            starved_gops: self.starved_gops,
            remedy_applied: remedy,
            recovery_ms,
        };

        IncidentChunk {
            chunk_id,
            time_window: (self.start_ms, end_ms),
            camera_id: self.camera_id,
            zone_id: self.zone_id,
            severity: self.severity,
            tags: self.tags,
            narrative,
            telemetry,
        }
    }
}

/// Key identifying an active coalescing window.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WindowKey {
    camera_id: CameraId,
    zone_id: Option<String>,
}

/// Sliding-window temporal coalescer.
///
/// Collects high-frequency telemetry events and rolls them into synthesized
/// [`IncidentChunk`] records upon window expiration or severity escalation.
#[derive(Debug)]
pub struct IncidentCoalescer {
    window_ms: u64,
    max_open_windows: usize,
    open_windows: BTreeMap<WindowKey, OpenWindow>,
}

impl IncidentCoalescer {
    /// Create a new coalescer with the given window duration and max concurrent window capacity.
    pub fn new(window_ms: u64, max_open_windows: usize) -> Self {
        Self {
            window_ms: window_ms.max(1_000),
            max_open_windows: max_open_windows.max(1),
            open_windows: BTreeMap::new(),
        }
    }

    /// Ingest a motion detection event.
    pub fn ingest_motion(&mut self, camera_id: &CameraId, event: &MotionEvent, now_ms: u64) {
        let key = WindowKey {
            camera_id: camera_id.clone(),
            zone_id: Some(event.zone_id.clone()),
        };
        self.ensure_capacity(now_ms);

        let window = self.open_windows.entry(key).or_insert_with(|| {
            OpenWindow::new(camera_id.clone(), Some(event.zone_id.clone()), now_ms)
        });

        window.last_update_ms = now_ms;
        window.motion_count = window.motion_count.saturating_add(1);
        if event.confidence > window.peak_confidence {
            window.peak_confidence = event.confidence;
        }

        if window.motion_count >= 10 && window.severity < IncidentSeverity::Warning {
            window.severity = IncidentSeverity::Warning;
            window.add_tag("motion_burst");
        } else {
            window.add_tag("motion");
        }
    }

    /// Ingest a reconciler or system audit entry.
    pub fn ingest_audit(&mut self, entry: &AuditEntry, now_ms: u64) {
        let cam_id = CameraId::new(&entry.camera_id);
        let key = WindowKey {
            camera_id: cam_id.clone(),
            zone_id: None,
        };
        self.ensure_capacity(now_ms);

        let window = self
            .open_windows
            .entry(key)
            .or_insert_with(|| OpenWindow::new(cam_id, None, now_ms));

        window.last_update_ms = now_ms;
        window
            .audit_events
            .push(format!("grade {}: {}", entry.grade, entry.what));

        let what_lower = entry.what.to_ascii_lowercase();
        if what_lower.contains("401") || what_lower.contains("auth") {
            window.add_tag("auth_failure");
            window.severity = window.severity.max(IncidentSeverity::Degraded);
        }
        if what_lower.contains("clock") || what_lower.contains("skew") {
            window.add_tag("clock_skew");
            window.severity = window.severity.max(IncidentSeverity::Degraded);
        }
        if what_lower.contains("offline") || what_lower.contains("unreachable") {
            window.add_tag("offline");
            window.severity = window.severity.max(IncidentSeverity::Critical);
        }
        if what_lower.contains("reset") {
            window.add_tag("session_reset");
        }
        if entry.grade >= 3 {
            window.severity = window.severity.max(IncidentSeverity::Critical);
        } else if entry.grade == 2 {
            window.severity = window.severity.max(IncidentSeverity::Degraded);
        } else if entry.grade == 1 {
            window.severity = window.severity.max(IncidentSeverity::Warning);
        }
    }

    /// Ingest transport layer health metrics (e.g. from `FrameReader` or RTP depacketizer).
    pub fn ingest_transport_metrics(
        &mut self,
        camera_id: &CameraId,
        _fps: f32,
        dropped_frames: u64,
        starved_gops: u64,
        now_ms: u64,
    ) {
        let key = WindowKey {
            camera_id: camera_id.clone(),
            zone_id: None,
        };
        self.ensure_capacity(now_ms);

        let window = self
            .open_windows
            .entry(key)
            .or_insert_with(|| OpenWindow::new(camera_id.clone(), None, now_ms));

        window.last_update_ms = now_ms;
        window.dropped_frames = window.dropped_frames.saturating_add(dropped_frames);
        window.starved_gops = window.starved_gops.saturating_add(starved_gops);

        if starved_gops > 0 {
            window.add_tag("gop_starvation");
            window.severity = window.severity.max(IncidentSeverity::Degraded);
        }
        if dropped_frames > 20 {
            window.add_tag("frame_drops");
            window.severity = window.severity.max(IncidentSeverity::Warning);
        }
    }

    /// Flush all open windows whose duration has exceeded `window_ms`, or that have reached `Critical` severity.
    pub fn flush_expired(&mut self, now_ms: u64) -> Vec<IncidentChunk> {
        let mut expired_keys = Vec::new();

        for (key, window) in &self.open_windows {
            let elapsed = now_ms.saturating_sub(window.start_ms);
            if elapsed >= self.window_ms || window.severity == IncidentSeverity::Critical {
                expired_keys.push(key.clone());
            }
        }

        let mut chunks = Vec::with_capacity(expired_keys.len());
        for key in expired_keys {
            if let Some(window) = self.open_windows.remove(&key) {
                chunks.push(window.synthesize(now_ms));
            }
        }

        chunks
    }

    /// Enforce bounded memory by evicting the oldest open window if capacity is reached.
    fn ensure_capacity(&mut self, now_ms: u64) {
        if self.open_windows.len() < self.max_open_windows {
            return;
        }

        // Find key with earliest start_ms
        let oldest_key = self
            .open_windows
            .iter()
            .min_by_key(|(_, w)| w.start_ms)
            .map(|(k, _)| k.clone());

        if let Some(k) = oldest_key {
            // Force synthesize and drop to prevent unbounded growth
            if let Some(window) = self.open_windows.remove(&k) {
                let _ = window.synthesize(now_ms);
            }
        }
    }

    /// The number of currently open tracking windows.
    pub fn open_window_count(&self) -> usize {
        self.open_windows.len()
    }
}

impl Default for IncidentCoalescer {
    fn default() -> Self {
        Self::new(60_000, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_motion_coalescing_and_expiration() {
        let mut coalescer = IncidentCoalescer::new(10_000, 10);
        let cam_id = CameraId::new("cam-1");

        let event1 = MotionEvent {
            camera_id: "cam-1".to_string(),
            zone_id: "loading-dock".to_string(),
            ts: 100,
            confidence: 0.85,
        };
        let event2 = MotionEvent {
            camera_id: "cam-1".to_string(),
            zone_id: "loading-dock".to_string(),
            ts: 101,
            confidence: 0.95,
        };

        coalescer.ingest_motion(&cam_id, &event1, 100_000);
        coalescer.ingest_motion(&cam_id, &event2, 102_000);

        assert_eq!(coalescer.open_window_count(), 1);

        // Before expiration
        let flushed = coalescer.flush_expired(105_000);
        assert!(flushed.is_empty());

        // After window expiration
        let flushed = coalescer.flush_expired(110_001);
        assert_eq!(flushed.len(), 1);
        let chunk = &flushed[0];
        assert_eq!(chunk.camera_id, cam_id);
        assert_eq!(chunk.zone_id.as_deref(), Some("loading-dock"));
        assert_eq!(chunk.telemetry.motion_count, 2);
        assert!((chunk.telemetry.peak_confidence - 0.95).abs() < 1e-4);
        assert!(chunk.narrative.contains("Recorded 2 motion detections"));
    }

    #[test]
    fn test_critical_audit_forces_immediate_flush() {
        let mut coalescer = IncidentCoalescer::new(60_000, 10);

        let audit = AuditEntry {
            camera_id: "cam-2".to_string(),
            what: "camera offline and unreachable".to_string(),
            grade: 3,
            at_ms: 1_000,
        };

        coalescer.ingest_audit(&audit, 1_000);
        let flushed = coalescer.flush_expired(2_000);
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].severity, IncidentSeverity::Critical);
        assert!(flushed[0].tags.contains(&"offline".to_string()));
    }

    #[test]
    fn test_capacity_bound_eviction() {
        let mut coalescer = IncidentCoalescer::new(60_000, 2);
        let cam1 = CameraId::new("cam-1");
        let cam2 = CameraId::new("cam-2");
        let cam3 = CameraId::new("cam-3");

        let event = MotionEvent {
            camera_id: "cam".to_string(),
            zone_id: "z".to_string(),
            ts: 100,
            confidence: 0.8,
        };

        coalescer.ingest_motion(&cam1, &event, 1_000);
        coalescer.ingest_motion(&cam2, &event, 2_000);
        assert_eq!(coalescer.open_window_count(), 2);

        // Ingesting third should evict oldest (cam1)
        coalescer.ingest_motion(&cam3, &event, 3_000);
        assert_eq!(coalescer.open_window_count(), 2);
    }
}
