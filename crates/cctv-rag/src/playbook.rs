//! Embedded protocol and vendor diagnostic playbooks.
//!
//! Provides troubleshooting knowledge chunks for common ONVIF, RTSP, RTP, and hardware
//! anomalies (e.g. clock-skew, digest nonce expiration, GOP starvation, PTZ watchdog runaway).

use serde::{Deserialize, Serialize};

/// A troubleshooting guide or diagnostic playbook chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticPlaybook {
    /// Unique identifier for the playbook rule (e.g. `onvif_wsse_clock_skew`).
    pub playbook_id: String,
    /// Protocol or subsystem (`onvif`, `rtsp`, `rtp`, `ptz`, `stream`).
    pub subsystem: String,
    /// Observable symptom description.
    pub symptom: String,
    /// Root cause analysis.
    pub root_cause: String,
    /// Step-by-step remediation recommendation.
    pub remediation: String,
    /// Searchable tags for keyword matching.
    pub tags: Vec<String>,
}

impl DiagnosticPlaybook {
    /// Generate a narrative string for BM25 indexing and LLM prompt context.
    pub fn to_narrative(&self) -> String {
        format!(
            "Diagnostic Playbook [{}] (Subsystem: {}): Symptom: {}. Root cause: {}. Recommended remediation: {}.",
            self.playbook_id, self.subsystem, self.symptom, self.root_cause, self.remediation
        )
    }
}

/// Registry of diagnostic playbooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookRegistry {
    playbooks: Vec<DiagnosticPlaybook>,
}

impl PlaybookRegistry {
    /// Create a new playbook registry initialized with built-in standard troubleshooting playbooks.
    pub fn new() -> Self {
        let mut registry = Self {
            playbooks: Vec::new(),
        };
        registry.register_builtins();
        registry
    }

    /// Register a custom playbook.
    pub fn register(&mut self, playbook: DiagnosticPlaybook) {
        self.playbooks
            .retain(|p| p.playbook_id != playbook.playbook_id);
        self.playbooks.push(playbook);
    }

    /// Access all registered playbooks.
    pub fn all(&self) -> &[DiagnosticPlaybook] {
        &self.playbooks
    }

    /// Find playbooks matching any of the given tags or search terms.
    pub fn find_by_tag(&self, tag: &str) -> Vec<&DiagnosticPlaybook> {
        let tag_lower = tag.to_ascii_lowercase();
        self.playbooks
            .iter()
            .filter(|p| {
                p.tags.iter().any(|t| t.to_ascii_lowercase() == tag_lower)
                    || p.subsystem.to_ascii_lowercase() == tag_lower
            })
            .collect()
    }

    fn register_builtins(&mut self) {
        self.register(DiagnosticPlaybook {
            playbook_id: "onvif_wsse_clock_skew".to_string(),
            subsystem: "onvif".to_string(),
            symptom: "ONVIF SOAP requests return wsse:FailedAuthentication or are dropped silently by camera.".to_string(),
            root_cause: "Timestamp delta between host system clock and camera clock exceeds 300 seconds, causing WS-Security UsernameToken Created validation failure.".to_string(),
            remediation: "Execute ClockHealing: probe camera HTTP Date response header, calculate clock drift offset, and apply offset delta to all outgoing SOAP WS-Security headers via RemedyStep::ClockHealing.".to_string(),
            tags: vec!["onvif".to_string(), "wsse".to_string(), "clock_skew".to_string(), "auth".to_string()],
        });

        self.register(DiagnosticPlaybook {
            playbook_id: "rtsp_digest_nonce_stale".to_string(),
            subsystem: "rtsp".to_string(),
            symptom: "Repeated RTSP 401 Unauthorized responses on DESCRIBE or SETUP despite valid credentials.".to_string(),
            root_cause: "Camera digest authentication session expired or nonce count (nc) desynchronized following network drop or camera reboot.".to_string(),
            remediation: "Flush session digest cache, send an initial unauthenticated DESCRIBE request to negotiate a fresh server nonce and realm, then recompute MD5/SHA256 digest response.".to_string(),
            tags: vec!["rtsp".to_string(), "digest".to_string(), "401".to_string(), "auth".to_string(), "nonce".to_string()],
        });

        self.register(DiagnosticPlaybook {
            playbook_id: "rtp_udp_gop_starvation".to_string(),
            subsystem: "rtp".to_string(),
            symptom: "FrameReader FPS drops to 0 or keyframe decode errors occur; UDP packet drops surge during scene changes.".to_string(),
            root_cause: "Oversized H.264/H.265 IDR keyframe fragmentation units (FU-A/FU) overflow host UDP receive socket buffers, dropping critical NAL units.".to_string(),
            remediation: "Reconfigure RTSP transport to interleaved TCP (channel $00) to ensure zero-loss packet delivery, or request shorter GOP intervals with lower bitrate profile from camera encoder.".to_string(),
            tags: vec!["rtp".to_string(), "udp".to_string(), "gop_starvation".to_string(), "dropped_frames".to_string(), "h264".to_string(), "h265".to_string()],
        });

        self.register(DiagnosticPlaybook {
            playbook_id: "ptz_watchdog_timeout".to_string(),
            subsystem: "ptz".to_string(),
            symptom: "Motorized camera continues moving indefinitely past intended pan/tilt coordinate or runs against physical limits.".to_string(),
            root_cause: "Client dropped connection or crashed before transmitting explicit PTZ stop command following a continuous move velocity command.".to_string(),
            remediation: "PtzWatchdog timer expires: automatically issue an imperative PtzCommand::Stop to the camera backend and log a warning remediation.".to_string(),
            tags: vec!["ptz".to_string(), "watchdog".to_string(), "timeout".to_string(), "runaway".to_string()],
        });

        self.register(DiagnosticPlaybook {
            playbook_id: "stream_budget_exhausted".to_string(),
            subsystem: "stream".to_string(),
            symptom: "Camera returns RTSP 453 Not Enough Bandwidth or refuses new TCP connection when opening additional stream.".to_string(),
            root_cause: "Hardware DSP on camera reached maximum concurrent RTSP sessions (typically 2-4 unicast sessions max).".to_string(),
            remediation: "Enforce StreamMux single-pull architecture: multiplex the single sub-stream RTSP feed into a local broadcast channel with GopCache, or serve image polls through SnapshotProxy.".to_string(),
            tags: vec!["stream".to_string(), "budget".to_string(), "453".to_string(), "bandwidth".to_string(), "stream_mux".to_string()],
        });
    }
}

impl Default for PlaybookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_playbooks_registered() {
        let registry = PlaybookRegistry::new();
        assert!(registry.all().len() >= 5);

        let clock_playbooks = registry.find_by_tag("clock_skew");
        assert_eq!(clock_playbooks.len(), 1);
        assert_eq!(clock_playbooks[0].playbook_id, "onvif_wsse_clock_skew");
        assert!(clock_playbooks[0].to_narrative().contains("ClockHealing"));
    }

    #[test]
    fn test_custom_playbook_registration() {
        let mut registry = PlaybookRegistry::new();
        registry.register(DiagnosticPlaybook {
            playbook_id: "custom_lens_blur".to_string(),
            subsystem: "optics".to_string(),
            symptom: "Image blurry".to_string(),
            root_cause: "Condensation on lens".to_string(),
            remediation: "Activate camera internal defogger heater.".to_string(),
            tags: vec!["optics".to_string(), "blur".to_string()],
        });

        let found = registry.find_by_tag("blur");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].playbook_id, "custom_lens_blur");
    }
}
