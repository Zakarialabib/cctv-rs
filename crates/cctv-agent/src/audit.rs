//! Audit trail for every reconciler change.
//!
//! Event table: `camera_id`, `what`, `grade`, `at`. One row per remediation.

pub struct Audit {
    // Hosted by the app's DB layer. `cctv-core` never touches SQLite.
}

impl Audit {
    pub fn new() -> Self {
        Self {}
    }

    pub fn log(&mut self, camera_id: String, what: &str, grade: u8) {
        // Implementation lives in the app's events store (writable at runtime).
    }
}
