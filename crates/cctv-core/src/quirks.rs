//! Hardware quirks data: firmware/device defects that the reconciler must work
//! around. Data, not code — the reconciler reads this and applies the remedy.
//!
//! Convention: `quirks/<vendor>/<model>.json`. The reconciler merges the JSON on
//! top of the device model.

pub fn quirks_dir() -> &'static str {
    // Hosted by the reconciler (or `cctv-core/quirks/` until we ship a file
    // loader). The driver crates keep the raw data files; this module is the
    // consumer of the data.
    "quirks/"
}

/// Return the remedy for a known defect, or `None` for unknown.
pub fn remedy(device: &str, defect: &str) -> Option<&'static str> {
    // Stub.
    None
}
