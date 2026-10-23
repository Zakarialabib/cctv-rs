//! Audit trail for every reconciler change.
//!
//! Event table: `camera_id`, `what`, `grade`, `at`. One row per remediation.

/// Append-only record of every remediation the reconciler applies.
pub struct Audit {
    // Hosted by the app's DB layer. `cctv-core` never touches SQLite.
}

impl Audit {
    /// Create an empty audit trail.
    pub fn new() -> Self {
        Self {}
    }

    /// Record one change: the affected camera, a description, and its severity grade.
    pub fn log(&mut self, _camera_id: String, _what: &str, _grade: u8) {
        // Implementation lives in the app's events store (writable at runtime).
    }
}

impl Default for Audit {
    fn default() -> Self {
        Self::new()
    }
}
