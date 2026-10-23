//! Reconciler: desired state vs observed state.
//!
//! 1. Read desired (dashboard).
//! 2. Read observed (via `cctv-core` + drivers).
//! 3. Diff.
//! 4. Apply graded remediation.
//!
//! Everything here is runtime-only. No commands cross into `cctv-core`.

use crate::audit::Audit;
use cctv_core::{CameraDesired, CameraState, CameraStateCurrent, CameraType};

pub struct Reconciler {
    audit: Audit,
}

impl Reconciler {
    pub fn new(audit: Audit) -> Self {
        Self { audit }
    }

    pub fn reconcile(&mut self, camera: &mut CameraState) {
        // Desired vs observed diff.
        // Produce a graded remediation from the cctv-core error taxonomy.
        self.audit.log(camera.id.clone(), "reconcile", 1);
    }
}
