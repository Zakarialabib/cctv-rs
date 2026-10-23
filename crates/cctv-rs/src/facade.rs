//! The facade gate.
//!
//! What SignageMaster's `cctv.ts` talks to. One `Facade` handle, no vendor names.
//! All of this lives in `cctv-core` types; drivers only settle them.

pub struct Facade {
    // Hosted by the app. The reconciler, drivers and frame reader are injected.
}

impl Facade {
    pub fn new() -> Self {
        Self {}
    }

    pub fn list_cameras(&self) -> Vec<CameraHandle> {
        vec![]
    }
}
