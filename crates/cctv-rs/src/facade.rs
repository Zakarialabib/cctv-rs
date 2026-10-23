//! The facade gate.
//!
//! What SignageMaster's `cctv.ts` talks to. One `Facade` handle, no vendor names.
//! All of this lives in `cctv-core` types; drivers only settle them.

use cctv_core::CameraHandle;

/// The single handle the application holds onto; it fronts the whole camera stack.
pub struct Facade {
    // Hosted by the app. The reconciler, drivers and frame reader are injected.
}

impl Facade {
    /// Create an empty facade for the app to wire with reconciler, drivers and frame reader.
    pub fn new() -> Self {
        Self {}
    }

    /// Return the currently known cameras as device-model handles.
    pub fn list_cameras(&self) -> Vec<CameraHandle> {
        vec![]
    }
}

impl Default for Facade {
    fn default() -> Self {
        Self::new()
    }
}
