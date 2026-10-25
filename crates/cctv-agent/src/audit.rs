//! Audit trail for every reconciler change.
//!
//! Append-only: one row per remediation, in the order it was applied.  The store
//! is in memory here; the application persists it, because the workspace owns no
//! database.

/// One recorded remediation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// The camera the remediation applied to.
    pub camera_id: String,
    /// What the reconciler did, in one line.
    pub what: String,
    /// The remediation's severity grade, as recorded.
    pub grade: u8,
    /// When it was applied, in Unix **milliseconds**.
    pub at_ms: u64,
}

/// Append-only record of every remediation the reconciler applies.
#[derive(Debug, Default)]
pub struct Audit {
    entries: Vec<AuditEntry>,
}

impl Audit {
    /// Create an empty audit trail.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one change: the affected camera, a description, and its grade.
    pub fn log(&mut self, camera_id: String, what: &str, grade: u8) {
        self.entries.push(AuditEntry {
            camera_id,
            what: what.to_string(),
            grade,
            at_ms: now_ms(),
        });
    }

    /// Every entry, in the order it was recorded.
    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    /// Every entry for one camera, oldest first.
    pub fn entries_for(&self, camera_id: &str) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.camera_id == camera_id)
            .collect()
    }

    /// How many entries have been recorded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Milliseconds since the Unix epoch (0 if the clock reads before it).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_append_only_and_filterable_by_camera() {
        let mut audit = Audit::new();
        assert!(audit.is_empty());

        audit.log("cam-a".into(), "retry", 3);
        audit.log("cam-b".into(), "hold", 0);
        audit.log("cam-a".into(), "notify", 2);

        assert_eq!(audit.len(), 3);
        assert_eq!(audit.entries()[0].what, "retry");
        assert_eq!(audit.entries()[2].grade, 2);

        let for_a = audit.entries_for("cam-a");
        assert_eq!(for_a.len(), 2);
        assert_eq!(for_a[0].what, "retry");
        assert_eq!(for_a[1].what, "notify");
        assert!(audit.entries_for("cam-z").is_empty());
    }

    #[test]
    fn an_entry_carries_a_timestamp() {
        let mut audit = Audit::new();
        audit.log("cam".into(), "x", 1);
        assert!(audit.entries()[0].at_ms > 0);
    }
}
