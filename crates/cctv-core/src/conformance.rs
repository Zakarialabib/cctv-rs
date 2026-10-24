//! The conformance suite every [`CameraBackend`] must pass.
//!
//! The suite is plain async code over the trait object, so it runs under any
//! executor: `cctv-core` tests drive it against [`crate::mock::MockCamera`]
//! with a `std`-only executor, and each driver crate drives it under its own
//! runtime against a fixture HTTP server.  It never panics — every failed
//! expectation is collected into the [`ConformanceReport`].

use crate::backend::{CameraBackend, PtzCommand, StreamQuality};
use crate::error::CameraError;

/// What the backend under test claims to support.
///
/// A capability that is *not* claimed must fail with a typed error, never
/// succeed silently and never panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expectations {
    /// The backend can capture snapshots.
    pub snapshot: bool,
    /// The backend can move the camera.
    pub ptz: bool,
    /// The backend can list / save / go to / delete presets.
    pub presets: bool,
}

impl Expectations {
    /// Everything supported.
    pub const FULL: Self = Self {
        snapshot: true,
        ptz: true,
        presets: true,
    };
}

/// The outcome of one suite run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConformanceReport {
    /// Names of the checks that passed.
    pub passed: Vec<&'static str>,
    /// Names of the checks that failed, each with a reason.
    pub failures: Vec<(&'static str, String)>,
}

impl ConformanceReport {
    /// True when no check failed.
    pub fn is_ok(&self) -> bool {
        self.failures.is_empty()
    }

    fn check(&mut self, name: &'static str, outcome: Result<(), String>) {
        match outcome {
            Ok(()) => self.passed.push(name),
            Err(reason) => self.failures.push((name, reason)),
        }
    }
}

impl core::fmt::Display for ConformanceReport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} passed, {} failed",
            self.passed.len(),
            self.failures.len()
        )?;
        for (name, reason) in &self.failures {
            write!(f, "\n  {name}: {reason}")?;
        }
        Ok(())
    }
}

fn unsupported(e: &CameraError) -> bool {
    matches!(e, CameraError::NotImplemented | CameraError::PtzDisabled)
}

/// Run every check against `backend`.
pub async fn run(backend: &dyn CameraBackend, expect: Expectations) -> ConformanceReport {
    let mut report = ConformanceReport::default();

    report.check(
        "driver_key_is_named",
        if backend.driver().is_empty() {
            Err("driver() returned an empty key".into())
        } else {
            Ok(())
        },
    );

    let probe = backend.probe().await;
    report.check(
        "probe_reports_identity",
        match &probe {
            Ok(p) if p.vendor.is_empty() => Err("probe vendor is empty".into()),
            Ok(p) if p.model.is_empty() => Err("probe model is empty".into()),
            Ok(_) => Ok(()),
            Err(e) => Err(format!("probe failed: {e}")),
        },
    );

    let main = backend.stream(StreamQuality::Main).await;
    report.check(
        "main_stream_resolves",
        match &main {
            Ok(s) if !s.uri.contains("://") => Err(format!("not a URI: {}", s.redacted_uri())),
            Ok(s) if s.quality != StreamQuality::Main => Err("quality is not Main".into()),
            Ok(_) => Ok(()),
            Err(e) => Err(format!("stream(Main) failed: {e}")),
        },
    );

    let sub = backend.stream(StreamQuality::Sub).await;
    report.check(
        "sub_stream_resolves_or_is_typed",
        match &sub {
            Ok(s) if !s.uri.contains("://") => Err(format!("not a URI: {}", s.redacted_uri())),
            Ok(_) => Ok(()),
            Err(e) if unsupported(e) => Ok(()),
            Err(e) => Err(format!("stream(Sub) failed: {e}")),
        },
    );

    let snap = backend.snapshot().await;
    report.check(
        "snapshot_matches_capability",
        match (&snap, expect.snapshot) {
            (Ok(s), true) if s.bytes.is_empty() => Err("snapshot is empty".into()),
            (Ok(_), true) => Ok(()),
            (Err(e), true) => Err(format!("snapshot failed: {e}")),
            (Ok(_), false) => Err("snapshot succeeded but is not claimed".into()),
            (Err(e), false) if unsupported(e) => Ok(()),
            (Err(e), false) => Err(format!("unclaimed snapshot gave untyped error: {e}")),
        },
    );

    let moved = backend
        .ptz(PtzCommand::Continuous {
            pan: 0.5,
            tilt: 0.0,
            zoom: 0.0,
        })
        .await;
    let stopped = backend.ptz(PtzCommand::Stop).await;
    report.check(
        "ptz_matches_capability",
        match (moved, stopped, expect.ptz) {
            (Ok(()), Ok(()), true) => Ok(()),
            (Err(e), _, true) | (_, Err(e), true) => Err(format!("ptz failed: {e}")),
            (Err(a), Err(b), false) if unsupported(&a) && unsupported(&b) => Ok(()),
            _ => Err("unclaimed ptz did not fail with a typed error".into()),
        },
    );

    if expect.presets {
        report.check("preset_round_trip", preset_round_trip(backend).await);
    } else {
        report.check(
            "unclaimed_presets_are_typed",
            match backend.presets().await {
                Err(e) if unsupported(&e) => Ok(()),
                Err(e) => Err(format!("untyped error: {e}")),
                Ok(_) => Err("presets succeeded but are not claimed".into()),
            },
        );
    }

    report
}

async fn preset_round_trip(backend: &dyn CameraBackend) -> Result<(), String> {
    backend
        .presets()
        .await
        .map_err(|e| format!("presets() failed: {e}"))?;
    let saved = backend
        .save_preset("conformance", None)
        .await
        .map_err(|e| format!("save_preset failed: {e}"))?;
    if saved.id.is_empty() {
        return Err("saved preset has no id".into());
    }
    if saved.name != "conformance" {
        return Err(format!("saved preset name is {:?}", saved.name));
    }
    backend
        .goto_preset(&saved.id)
        .await
        .map_err(|e| format!("goto_preset failed: {e}"))?;
    backend
        .delete_preset(&saved.id)
        .await
        .map_err(|e| format!("delete_preset failed: {e}"))?;
    Ok(())
}
