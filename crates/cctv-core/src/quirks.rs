//! Hardware quirks data: firmware / device defects that the reconciler must
//! work around.  Data, not code — the reconciler reads this config and applies
//! the remedy.
//!
//! Convention: `quirks/<vendor>/<model>.json`.  The driver crates keep the raw
//! data files; this module is the consumer of the data (and the place the
//! default remediation lives).

/// Directory where quirks are loaded.  Hosted by the reconciler (or
/// `cctv-core/quirks/` until we ship a file loader).  The driver crates keep
/// the raw data files; this module is the consumer of the data.
pub fn quirks_dir() -> &'static str {
    "quirks/"
}

/// Return the remedy for a known defect, or `None` for unknown.
pub fn remedy(_device: &str, _defect: &str) -> Option<&'static str> {
    // Concrete vendors override this with a loaded JSON map.  The data model
    // stays pure: no vendor appears here, only the data files.
    None
}
