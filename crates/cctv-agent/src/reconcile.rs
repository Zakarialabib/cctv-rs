//! Reconciler: desired state vs observed state.
//!
//! 1. Read desired (the dashboard's intent).
//! 2. Read observed (a `cctv-core` `CameraState`, fed by a driver probe).
//! 3. Diff.
//! 4. Apply a **graded** remediation, and record it.
//!
//! The grade is the point.  A camera that is merely unreachable earns a *retry*,
//! one that failed authentication earns a *reconfigure* (only an operator can
//! fix it), and one the fleet no longer has earns a *disable*.  An error the
//! reconciler does not recognise is *notify* — never *disable*, because an
//! unknown failure is not evidence that the camera is gone.
//!
//! Everything here is runtime-only.  No commands cross into `cctv-core`.

use std::collections::BTreeMap;

use cctv_core::{CameraDesired, CameraError, CameraState, CameraStateCurrent};

use crate::audit::Audit;

/// Below this confidence the observation is not trusted enough to act on.
const CONFIDENCE_FLOOR: f32 = 0.3;

/// How invasive a remediation is.
///
/// Higher grades are more disruptive, so the reconciler always prefers the
/// lowest grade that resolves the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grade {
    /// The camera matches its desired state; nothing to do.
    None = 0,
    /// Record it and keep watching — expected, not a fault.
    Observe = 1,
    /// Tell the operator; the reconciler will not act on its own.
    Notify = 2,
    /// Try again: reconnect, or restart the stream.
    Retry = 3,
    /// Change the camera's configuration.
    Reconfigure = 4,
    /// Take the camera out of service.
    Disable = 5,
}

impl Grade {
    /// The numeric grade recorded in the audit trail.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl core::fmt::Display for Grade {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match self {
            Self::None => "none",
            Self::Observe => "observe",
            Self::Notify => "notify",
            Self::Retry => "retry",
            Self::Reconfigure => "reconfigure",
            Self::Disable => "disable",
        };
        f.write_str(name)
    }
}

/// What a remediation does to the camera's observed state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Leave the camera alone.
    Hold,
    /// Step the live state to this value.
    Advance(CameraStateCurrent),
    /// Record the camera as offline.
    MarkOffline,
    /// Take the camera out of service.
    TakeOutOfService,
}

/// One graded remediation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remediation {
    /// The camera it applies to.
    pub camera_id: String,
    /// How invasive it is.
    pub grade: Grade,
    /// What it does.
    pub action: Action,
    /// Why, in one line.
    pub reason: String,
}

impl Remediation {
    /// Whether this remediation changed anything.
    pub fn is_actionable(&self) -> bool {
        self.grade != Grade::None
    }
}

/// Brings a fleet's observed state back to the desired state, recording each
/// remediation it applies.
#[derive(Debug, Default)]
pub struct Reconciler {
    audit: Audit,
    desired: BTreeMap<String, CameraDesired>,
    errors: BTreeMap<String, CameraError>,
}

impl Reconciler {
    /// Create a reconciler that records changes in `audit`.
    pub fn new(audit: Audit) -> Self {
        Self {
            audit,
            desired: BTreeMap::new(),
            errors: BTreeMap::new(),
        }
    }

    /// The audit trail this reconciler records into.
    pub fn audit(&self) -> &Audit {
        &self.audit
    }

    /// Take the audit trail back out, e.g. to persist it.
    pub fn into_audit(self) -> Audit {
        self.audit
    }

    /// Record the operator's intent for one camera.
    pub fn set_desired(&mut self, camera_id: impl Into<String>, desired: CameraDesired) {
        self.desired.insert(camera_id.into(), desired);
    }

    /// The operator's intent for one camera, if any.
    pub fn desired_for(&self, camera_id: &str) -> Option<CameraDesired> {
        self.desired.get(camera_id).copied()
    }

    /// Record the last error a probe or command produced for a camera.
    ///
    /// It is graded on the next reconciliation and then cleared, so a transient
    /// failure does not keep grading forever.
    pub fn record_error(&mut self, camera_id: impl Into<String>, error: CameraError) {
        self.errors.insert(camera_id.into(), error);
    }

    /// Forget the recorded error for a camera.
    pub fn clear_error(&mut self, camera_id: &str) {
        self.errors.remove(camera_id);
    }

    /// Diff desired against observed state for `camera`, apply the remediation,
    /// and record it.
    pub fn reconcile(&mut self, camera: &mut CameraState) -> Remediation {
        let desired = self
            .desired
            .get(&camera.id)
            .copied()
            .unwrap_or(camera.desired);
        camera.set_desired(desired);

        let remediation = match self.errors.remove(&camera.id) {
            Some(error) => grade_error(&camera.id, &error),
            None => diff(&camera.id, desired, camera),
        };
        apply(&remediation, camera);
        self.audit.log(
            remediation.camera_id.clone(),
            &format!("{}: {}", remediation.grade, remediation.reason),
            remediation.grade.as_u8(),
        );
        remediation
    }

    /// Reconcile a whole fleet, in camera-id order.
    pub fn reconcile_all(&mut self, fleet: &mut [CameraState]) -> Vec<Remediation> {
        fleet.sort_by(|a, b| a.id.cmp(&b.id));
        fleet
            .iter_mut()
            .map(|camera| self.reconcile(camera))
            .collect()
    }
}

/// Grade a failure from the `cctv-core` error taxonomy.
///
/// The grade is the *least invasive* response that can work, and the mapping is
/// total: every variant is accounted for, so no failure is silently ignored.
fn grade_error(camera_id: &str, error: &CameraError) -> Remediation {
    let (grade, action, reason) = match error {
        CameraError::Offline | CameraError::Network => (
            Grade::Retry,
            Action::MarkOffline,
            "not reachable; reconnect attempts are warranted",
        ),
        CameraError::StreamFailed => (Grade::Retry, Action::Hold, "the stream dropped"),
        CameraError::CaptureFailed => (Grade::Retry, Action::Hold, "capture failed"),
        CameraError::AuthFailed => (
            Grade::Reconfigure,
            Action::Hold,
            "authentication failed; only an operator can supply credentials",
        ),
        CameraError::Refused => (
            Grade::Notify,
            Action::Hold,
            "the device refused the command",
        ),
        // A capability the camera simply does not have is not a fault.
        CameraError::PtzDisabled | CameraError::NotImplemented => (
            Grade::Observe,
            Action::Hold,
            "capability not present on this camera",
        ),
        CameraError::NoCamera => (
            Grade::Disable,
            Action::TakeOutOfService,
            "no camera matches this identity",
        ),
        CameraError::Other(_) => (Grade::Notify, Action::Hold, "unclassified failure"),
    };
    Remediation {
        camera_id: camera_id.to_string(),
        grade,
        action,
        reason: format!("{reason} ({error})"),
    }
}

/// Diff the desired state against what the camera is observed to be doing.
fn diff(camera_id: &str, desired: CameraDesired, observed: &CameraState) -> Remediation {
    let remediation = |grade, action, reason: &str| Remediation {
        camera_id: camera_id.to_string(),
        grade,
        action,
        reason: reason.to_string(),
    };

    // 1. The operator's "disabled" always wins, and is the one intent that takes
    //    a camera out of service rather than trying to revive it.
    if desired == CameraDesired::Disabled {
        return if observed.state == CameraStateCurrent::Off {
            remediation(Grade::None, Action::Hold, "disabled, and already off")
        } else {
            remediation(
                Grade::Reconfigure,
                Action::TakeOutOfService,
                "the operator disabled this camera",
            )
        };
    }

    // 2. Unreachable: reconnect attempts are warranted, and the state is
    //    corrected before anything else is attempted.
    if !observed.online || observed.state == CameraStateCurrent::Away {
        return remediation(
            Grade::Retry,
            Action::MarkOffline,
            "the camera is not reachable",
        );
    }

    // 3. A low-confidence observation is not a basis for acting.
    if observed.confidence < CONFIDENCE_FLOOR {
        return remediation(
            Grade::Observe,
            Action::Hold,
            "confidence is below the floor; the observation is not trusted",
        );
    }

    // 4. Otherwise step the live state toward the desired one.
    let target = match desired {
        CameraDesired::Idle => CameraStateCurrent::On,
        CameraDesired::Armed => CameraStateCurrent::Streaming,
        CameraDesired::Recording => CameraStateCurrent::Recording,
        // Handled above; a belt-and-braces value rather than a panic, because
        // nothing in this crate may panic.
        CameraDesired::Disabled => CameraStateCurrent::Off,
    };
    if observed.state == target {
        return remediation(Grade::None, Action::Hold, "matches its desired state");
    }
    remediation(
        Grade::Notify,
        Action::Advance(target),
        "stepping the live state toward the desired one",
    )
}

/// Apply a remediation to the camera's observed state.
fn apply(remediation: &Remediation, camera: &mut CameraState) {
    match remediation.action {
        Action::Hold => {}
        Action::Advance(target) => camera.transition(target),
        Action::MarkOffline => {
            camera.online = false;
            camera.transition(CameraStateCurrent::Away);
        }
        Action::TakeOutOfService => camera.transition(CameraStateCurrent::Off),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctv_core::CameraType;

    /// A camera the fleet believes is online, on `state`, at `confidence`.
    fn observed(id: &str, state: CameraStateCurrent, confidence: f32) -> CameraState {
        let mut camera = CameraState::new(id.to_string(), "Vendor".into(), CameraType::Business);
        camera.online = true;
        camera.state = state;
        camera.confidence = confidence;
        camera
    }

    fn reconciler() -> Reconciler {
        Reconciler::new(Audit::new())
    }

    #[test]
    fn a_camera_that_matches_its_desire_holds() {
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);
        camera.set_desired(CameraDesired::Idle);
        let remediation = reconciler().reconcile(&mut camera);

        assert_eq!(remediation.grade, Grade::None);
        assert_eq!(remediation.action, Action::Hold);
        assert!(!remediation.is_actionable());
    }

    #[test]
    fn an_armed_camera_is_stepped_up_to_streaming() {
        let mut r = reconciler();
        r.set_desired("cam", CameraDesired::Armed);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);

        let remediation = r.reconcile(&mut camera);
        assert_eq!(remediation.grade, Grade::Notify);
        assert_eq!(
            remediation.action,
            Action::Advance(CameraStateCurrent::Streaming)
        );
        assert_eq!(camera.state, CameraStateCurrent::Streaming);
        assert_eq!(camera.desired, CameraDesired::Armed);
    }

    #[test]
    fn recording_and_idle_step_in_both_directions() {
        let mut r = reconciler();
        r.set_desired("cam", CameraDesired::Recording);
        let mut camera = observed("cam", CameraStateCurrent::Streaming, 1.0);
        r.reconcile(&mut camera);
        assert_eq!(camera.state, CameraStateCurrent::Recording);

        r.set_desired("cam", CameraDesired::Idle);
        r.reconcile(&mut camera);
        assert_eq!(camera.state, CameraStateCurrent::On);
    }

    #[test]
    fn an_unreachable_camera_is_marked_offline_and_retried() {
        let mut camera = CameraState::new("cam".into(), "Vendor".into(), CameraType::Business);
        camera.online = false;
        let remediation = reconciler().reconcile(&mut camera);

        assert_eq!(remediation.grade, Grade::Retry);
        assert_eq!(remediation.action, Action::MarkOffline);
        assert_eq!(camera.state, CameraStateCurrent::Away);
        assert!(!camera.online);
    }

    #[test]
    fn a_disabled_desire_takes_the_camera_out_of_service() {
        let mut r = reconciler();
        r.set_desired("cam", CameraDesired::Disabled);
        let mut camera = observed("cam", CameraStateCurrent::Streaming, 1.0);

        let remediation = r.reconcile(&mut camera);
        assert_eq!(remediation.grade, Grade::Reconfigure);
        assert_eq!(remediation.action, Action::TakeOutOfService);
        assert_eq!(camera.state, CameraStateCurrent::Off);

        // Reconciling again is a no-op: it is already off.
        let again = r.reconcile(&mut camera);
        assert_eq!(again.grade, Grade::None);
    }

    #[test]
    fn a_low_confidence_observation_is_not_acted_on() {
        let mut r = reconciler();
        r.set_desired("cam", CameraDesired::Armed);
        let mut camera = observed("cam", CameraStateCurrent::On, 0.1);

        let remediation = r.reconcile(&mut camera);
        assert_eq!(remediation.grade, Grade::Observe);
        assert_eq!(remediation.action, Action::Hold);
        assert_eq!(camera.state, CameraStateCurrent::On, "not advanced");
    }

    #[test]
    fn an_auth_failure_is_reconfigured_and_never_disabled() {
        let mut r = reconciler();
        r.record_error("cam", CameraError::AuthFailed);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);

        let remediation = r.reconcile(&mut camera);
        assert_eq!(remediation.grade, Grade::Reconfigure);
        assert_eq!(remediation.action, Action::Hold);
    }

    #[test]
    fn an_unknown_error_is_notified_never_disabled() {
        let mut r = reconciler();
        r.record_error("cam", CameraError::Other("gremlins".into()));
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);

        let remediation = r.reconcile(&mut camera);
        assert_eq!(remediation.grade, Grade::Notify);
        assert_ne!(remediation.grade, Grade::Disable);
    }

    #[test]
    fn a_missing_camera_is_the_one_grade_that_disables() {
        let mut r = reconciler();
        r.record_error("cam", CameraError::NoCamera);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);

        let remediation = r.reconcile(&mut camera);
        assert_eq!(remediation.grade, Grade::Disable);
        assert_eq!(remediation.action, Action::TakeOutOfService);
        assert_eq!(camera.state, CameraStateCurrent::Off);
    }

    #[test]
    fn a_missing_capability_is_observed_not_retried() {
        let mut r = reconciler();
        r.record_error("cam", CameraError::PtzDisabled);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);
        assert_eq!(r.reconcile(&mut camera).grade, Grade::Observe);

        r.record_error("cam", CameraError::NotImplemented);
        assert_eq!(r.reconcile(&mut camera).grade, Grade::Observe);
    }

    #[test]
    fn an_error_is_graded_once_and_then_cleared() {
        let mut r = reconciler();
        // The camera already matches its desire, so only the error can grade it.
        r.set_desired("cam", CameraDesired::Armed);
        r.record_error("cam", CameraError::StreamFailed);
        let mut camera = observed("cam", CameraStateCurrent::Streaming, 1.0);

        assert_eq!(r.reconcile(&mut camera).grade, Grade::Retry);
        // The second pass sees no error and the state matches, so nothing is left.
        assert_eq!(r.reconcile(&mut camera).grade, Grade::None);
    }

    #[test]
    fn the_audit_records_every_remediation_in_order() {
        let mut r = reconciler();
        r.set_desired("cam", CameraDesired::Armed);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);

        r.reconcile(&mut camera);
        r.set_desired("cam", CameraDesired::Disabled);
        r.reconcile(&mut camera);

        let entries = r.audit().entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].camera_id, "cam");
        assert_eq!(entries[0].grade, Grade::Notify.as_u8());
        assert!(entries[0].what.contains("notify"), "{}", entries[0].what);
        assert_eq!(entries[1].grade, Grade::Reconfigure.as_u8());
        assert!(entries[1].at_ms >= entries[0].at_ms);
    }

    #[test]
    fn the_fleet_is_swept_in_id_order() {
        let mut r = reconciler();
        r.set_desired("c", CameraDesired::Armed);
        r.set_desired("a", CameraDesired::Armed);
        r.set_desired("b", CameraDesired::Armed);

        let mut fleet = vec![
            observed("c", CameraStateCurrent::On, 1.0),
            observed("a", CameraStateCurrent::On, 1.0),
            observed("b", CameraStateCurrent::On, 1.0),
        ];
        let remediations = r.reconcile_all(&mut fleet);

        let ids: Vec<&str> = remediations.iter().map(|r| r.camera_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
        assert!(fleet
            .iter()
            .all(|c| c.state == CameraStateCurrent::Streaming));
    }

    #[test]
    fn the_desired_state_is_read_from_the_reconciler_not_the_camera() {
        let mut r = reconciler();
        r.set_desired("cam", CameraDesired::Recording);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);
        camera.set_desired(CameraDesired::Idle);

        r.reconcile(&mut camera);
        assert_eq!(
            camera.desired,
            CameraDesired::Recording,
            "the operator wins"
        );
        assert_eq!(r.desired_for("cam"), Some(CameraDesired::Recording));
        assert_eq!(r.desired_for("nope"), None);
    }

    #[test]
    fn grades_are_ordered_from_least_to_most_invasive() {
        assert!(Grade::None < Grade::Observe);
        assert!(Grade::Observe < Grade::Notify);
        assert!(Grade::Notify < Grade::Retry);
        assert!(Grade::Retry < Grade::Reconfigure);
        assert!(Grade::Reconfigure < Grade::Disable);
        assert_eq!(Grade::Disable.as_u8(), 5);
    }
}
