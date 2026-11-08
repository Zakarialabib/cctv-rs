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
//! Everything here is runtime-only.  A remediation *names* the `CommandId`s it
//! authorizes ([`Remediation::commands`]); issuing them is the caller's choice
//! through a [`CommandRelay`], so the reconciler itself opens no connection and
//! pulls no runtime into this crate.

use std::collections::BTreeMap;

use cctv_core::{
    CameraConfig, CameraDesired, CameraError, CameraObservation, CameraState, CameraStateCurrent,
    CommandId, DomainEvent, RecordingProfile,
};

use crate::audit::Audit;
use crate::clock::{Clock, SystemClock};
use crate::policy::{Policy, PolicyTracker, PolicyVerdict};
use std::sync::Arc;

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

    /// The [`CommandId`]s this remediation authorizes, in execution order.
    ///
    /// Derived from the action, gated on the grade:
    ///
    /// - [`Action::Hold`] issues nothing — holding *is* leaving the camera
    ///   alone, which is why an auth failure (reconfigure + hold) never
    ///   commands: only an operator can supply credentials.
    /// - [`Action::MarkOffline`] → `TestCameraConnection`, the reconnect probe
    ///   the grade promises.
    /// - [`Action::TakeOutOfService`] → `CameraSetEnabled`: disabling the
    ///   camera in the fleet is the configuration change itself.
    /// - [`Action::Advance`] steps toward `Streaming` / `Recording` →
    ///   `StartCameraStream`, and toward `Retrying` → `TestCameraConnection`.
    ///   These execute the operator's own `set_desired` intent, so they are
    ///   authorized by the desire, not "acting on its own" — `Notify`'s
    ///   prohibition covers escalation (reconfigure / disable).  Other
    ///   targets are belief steps with no wire command.
    ///
    /// A non-actionable grade (`Grade::None`) issues nothing.
    pub fn commands(&self) -> Vec<CommandId> {
        if !self.is_actionable() {
            return Vec::new();
        }
        match self.action {
            Action::Hold => Vec::new(),
            Action::MarkOffline => vec![CommandId::TestCameraConnection],
            Action::TakeOutOfService => vec![CommandId::CameraSetEnabled],
            Action::Advance(CameraStateCurrent::Streaming | CameraStateCurrent::Recording) => {
                vec![CommandId::StartCameraStream]
            }
            Action::Advance(CameraStateCurrent::Retrying) => {
                vec![CommandId::TestCameraConnection]
            }
            Action::Advance(_) => Vec::new(),
        }
    }
}

/// Where the reconciler sends the commands a remediation authorizes.
///
/// The seam that makes remediation *active*: `reconcile` issues each command
/// in [`Remediation::commands`] through the relay as it grades, instead of
/// only reporting the grade.  Sync by contract — `cctv-agent` has no runtime —
/// so an implementor queues for whoever does (the facade's async loop, a
/// channel to the application, a test's `Vec`).  `Send + Debug` are required:
/// the reconciler travels inside `Send` application state, and `Debug` keeps
/// `Reconciler`'s derive intact.
///
/// Modeled on `tv-rs`'s `Reconciler::with_command_relay`, which feeds an
/// imperative channel from the same grading pass.
pub trait CommandRelay: std::fmt::Debug + Send {
    /// Issue one remediation command for one camera.
    fn issue(&mut self, camera_id: &str, command: CommandId);
}

/// Brings a fleet's observed state back to the desired state, recording each
/// remediation it applies.
#[derive(Debug)]
pub struct Reconciler {
    audit: Audit,
    clock: Arc<dyn Clock>,
    desired: BTreeMap<String, CameraDesired>,
    errors: BTreeMap<String, CameraError>,
    group_profiles: BTreeMap<String, RecordingProfile>,
    last_seq: BTreeMap<String, u64>,
    /// Where remediation commands go, when the caller wants active issuance.
    relay: Option<Box<dyn CommandRelay>>,
    /// Active reconciliation policy governing automated remediations.
    policy: Option<Policy>,
    /// Policy runtime tracking state (cooldowns, action counts).
    policy_tracker: PolicyTracker,
}

impl Default for Reconciler {
    fn default() -> Self {
        Self::new(Audit::new())
    }
}

impl Reconciler {
    /// Create a reconciler that records changes in `audit`, using the default system clock.
    pub fn new(audit: Audit) -> Self {
        Self::with_clock(audit, Arc::new(SystemClock))
    }

    /// Create a reconciler with a custom clock.
    pub fn with_clock(audit: Audit, clock: Arc<dyn Clock>) -> Self {
        Self {
            audit,
            clock,
            desired: BTreeMap::new(),
            errors: BTreeMap::new(),
            group_profiles: BTreeMap::new(),
            last_seq: BTreeMap::new(),
            relay: None,
            policy: None,
            policy_tracker: PolicyTracker::new(),
        }
    }

    /// Set an active reconciliation policy on this reconciler.
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = Some(policy);
    }

    /// Builder method to configure an active reconciliation policy.
    #[must_use]
    pub fn with_policy(mut self, policy: Policy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Access the active policy, if configured.
    pub fn policy(&self) -> Option<&Policy> {
        self.policy.as_ref()
    }

    /// Access the policy runtime tracker mutably, e.g. for testing or resets.
    pub fn policy_tracker_mut(&mut self) -> &mut PolicyTracker {
        &mut self.policy_tracker
    }

    /// Assign a default composite recording profile to a camera group.
    pub fn set_group_profile(&mut self, group: impl Into<String>, profile: RecordingProfile) {
        self.group_profiles.insert(group.into(), profile);
    }

    /// Get the assigned composite recording profile for a camera group, if configured.
    pub fn group_profile(&self, group: &str) -> Option<&RecordingProfile> {
        self.group_profiles.get(group)
    }

    /// Reconcile a fleet of camera configurations against group recording profiles.
    ///
    /// If a camera has an assigned `group` and matches a registered group profile,
    /// sets `camera_config.recording_profile` to the group's profile and returns the list of
    /// updated camera IDs.
    pub fn reconcile_group_profiles(&mut self, configs: &mut [CameraConfig]) -> Vec<String> {
        let mut updated = Vec::new();
        for config in configs {
            if let Some(ref group) = config.group {
                if let Some(profile) = self.group_profiles.get(group) {
                    if config.recording_profile.as_ref() != Some(profile) {
                        config.recording_profile = Some(profile.clone());
                        updated.push(config.id.clone());
                        self.audit.log(
                            config.id.clone(),
                            &format!("applied recording profile from group '{group}'"),
                            Grade::Reconfigure.as_u8(),
                        );
                    }
                }
            }
        }
        updated
    }

    /// The audit trail this reconciler records into.
    pub fn audit(&self) -> &Audit {
        &self.audit
    }

    /// Take the audit trail back out, e.g. to persist it.
    pub fn into_audit(self) -> Audit {
        self.audit
    }

    /// Issue remediation commands through `relay` as they are graded
    /// (plan v0.2 W5 — active remediation).
    ///
    /// Without a relay the reconciler stays report-only: `Remediation::commands`
    /// still names what it would issue, so nothing is lost by not installing one.
    #[must_use]
    pub fn with_command_relay(mut self, relay: Box<dyn CommandRelay>) -> Self {
        self.relay = Some(relay);
        self
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

        let candidate = match self.errors.remove(&camera.id) {
            Some(error) => grade_error(&camera.id, &error),
            None => diff(&camera.id, desired, camera),
        };

        let (verdict, policy_id) = if let Some(ref policy) = self.policy {
            (
                policy.evaluate(candidate, self.clock.now_ms(), &mut self.policy_tracker),
                Some(policy.id.clone()),
            )
        } else {
            (PolicyVerdict::Apply(candidate), None)
        };

        let remediation = verdict.remediation().clone();
        if verdict.is_executable() {
            apply(&remediation, camera);
        }

        let audit_msg = match policy_id {
            Some(pid) => match &verdict {
                PolicyVerdict::DryRun { .. } => {
                    format!(
                        "[policy:{pid}] dry_run {}: {}",
                        remediation.grade, remediation.reason
                    )
                }
                PolicyVerdict::Suppressed { reason, .. } => {
                    format!(
                        "[policy:{pid}] suppressed ({reason}) {}: {}",
                        remediation.grade, remediation.reason
                    )
                }
                PolicyVerdict::Apply(_) => {
                    format!(
                        "[policy:{pid}] {}: {}",
                        remediation.grade, remediation.reason
                    )
                }
            },
            None => format!("{}: {}", remediation.grade, remediation.reason),
        };

        self.audit.log(
            remediation.camera_id.clone(),
            &audit_msg,
            remediation.grade.as_u8(),
        );

        if verdict.is_executable() {
            self.issue(&remediation);
        }
        remediation
    }

    /// Push a remediation's commands through the relay, auditing each issue.
    ///
    /// Nothing to do when no relay is installed or the remediation authorizes
    /// no command — the report-only path is unchanged.
    fn issue(&mut self, remediation: &Remediation) {
        let commands = remediation.commands();
        if commands.is_empty() {
            return;
        }
        let Some(relay) = self.relay.as_mut() else {
            return;
        };
        for command in commands {
            relay.issue(&remediation.camera_id, command);
            self.audit.log(
                remediation.camera_id.clone(),
                &format!("issued {command:?}"),
                remediation.grade.as_u8(),
            );
        }
    }

    /// Ingest a push-based [`CameraObservation`] directly without polling,
    /// updating `camera` state and returning the resulting [`Remediation`].
    ///
    /// Preserves strict per-camera ordering: observations with sequence
    /// numbers less than or equal to the latest processed for this camera are
    /// dropped, returning `None`.
    pub fn handle_observation(
        &mut self,
        observation: &CameraObservation,
        camera: &mut CameraState,
    ) -> Option<Remediation> {
        let last = self
            .last_seq
            .get(&observation.camera_id)
            .copied()
            .unwrap_or(0);
        if observation.sequence <= last {
            // Out of order or duplicate observation; drop to guarantee monotonic order
            return None;
        }
        self.last_seq
            .insert(observation.camera_id.clone(), observation.sequence);

        let desired = self
            .desired
            .get(&observation.camera_id)
            .copied()
            .unwrap_or(camera.desired);
        camera.set_desired(desired);

        // Update camera state machine from observation
        if let Some(ref probe) = observation.probe {
            camera.apply_probe(
                probe.clone(),
                observation.online,
                observation.state,
                observation.confidence,
            );
        } else {
            camera.update_confirmation(
                observation.online,
                observation.state,
                observation.confidence,
            );
        }

        let candidate = match self.errors.remove(&camera.id) {
            Some(error) => grade_error(&camera.id, &error),
            None => diff(&camera.id, desired, camera),
        };

        let (verdict, policy_id) = if let Some(ref policy) = self.policy {
            (
                policy.evaluate(candidate, self.clock.now_ms(), &mut self.policy_tracker),
                Some(policy.id.clone()),
            )
        } else {
            (PolicyVerdict::Apply(candidate), None)
        };

        let remediation = verdict.remediation().clone();
        if verdict.is_executable() {
            apply(&remediation, camera);
        }

        let audit_msg = match policy_id {
            Some(pid) => match &verdict {
                PolicyVerdict::DryRun { .. } => {
                    format!(
                        "[policy:{pid}] dry_run observe [seq:{}]: {}: {}",
                        observation.sequence, remediation.grade, remediation.reason
                    )
                }
                PolicyVerdict::Suppressed { reason, .. } => {
                    format!(
                        "[policy:{pid}] suppressed ({reason}) observe [seq:{}]: {}: {}",
                        observation.sequence, remediation.grade, remediation.reason
                    )
                }
                PolicyVerdict::Apply(_) => {
                    format!(
                        "[policy:{pid}] observe [seq:{}]: {}: {}",
                        observation.sequence, remediation.grade, remediation.reason
                    )
                }
            },
            None => format!(
                "observe [seq:{}]: {}: {}",
                observation.sequence, remediation.grade, remediation.reason
            ),
        };

        self.audit.log(
            remediation.camera_id.clone(),
            &audit_msg,
            remediation.grade.as_u8(),
        );

        if verdict.is_executable() {
            self.issue(&remediation);
        }
        Some(remediation)
    }

    /// Reconcile a whole fleet, in camera-id order.
    pub fn reconcile_all(&mut self, fleet: &mut [CameraState]) -> Vec<Remediation> {
        self.policy_tracker.begin_tick();
        fleet.sort_by(|a, b| a.id.cmp(&b.id));
        fleet
            .iter_mut()
            .map(|camera| self.reconcile(camera))
            .collect()
    }

    /// React to a [`DomainEvent`] emitted by the HAL (e.g. hot-plug attached/dropped).
    ///
    /// When `DomainEvent::DeviceDropped` is received, transitions the Aggregate Root to
    /// [`CameraStateCurrent::Disconnected`] and records a `CameraDropped` audit log entry
    /// preserving the historical context ("Ghost Device" audit).
    pub fn handle_event(&mut self, event: &DomainEvent, camera: &mut CameraState) -> Remediation {
        match event {
            DomainEvent::DeviceAttached { camera_id, driver } => {
                camera.online = true;
                camera.transition(CameraStateCurrent::Standby);
                let remediation = Remediation {
                    camera_id: camera_id.clone(),
                    grade: Grade::Observe,
                    action: Action::Advance(CameraStateCurrent::Standby),
                    reason: format!("device attached via driver {driver}"),
                };
                self.audit.log(
                    camera_id.clone(),
                    &format!("{}: {}", remediation.grade, remediation.reason),
                    remediation.grade.as_u8(),
                );
                remediation
            }
            DomainEvent::DeviceDropped { camera_id } => {
                camera.online = false;
                camera.transition(CameraStateCurrent::Disconnected);
                let remediation = Remediation {
                    camera_id: camera_id.clone(),
                    grade: Grade::Disable,
                    action: Action::MarkOffline,
                    reason: "CameraDropped: device hot-unplugged (ghost device preserved)"
                        .to_string(),
                };
                self.audit.log(
                    camera_id.clone(),
                    &format!(
                        "CameraDropped: {}: {}",
                        remediation.grade, remediation.reason
                    ),
                    remediation.grade.as_u8(),
                );
                remediation
            }
        }
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
        CameraError::DeviceBusy => (
            Grade::Retry,
            Action::Hold,
            "device busy; wait and retry (resource contention)",
        ),
        CameraError::SessionLimitExceeded => (
            Grade::Retry,
            Action::Hold,
            "session limit exceeded; wait for a session slot to free",
        ),
        CameraError::MediaUnavailable => (
            Grade::Observe,
            Action::Hold,
            "media unavailable; defer until storage ready",
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
        CameraError::PtzDisabled | CameraError::NotImplemented | CameraError::Unsupported(_) => (
            Grade::Observe,
            Action::Hold,
            "capability not present on this camera",
        ),
        CameraError::NoCamera => (
            Grade::Disable,
            Action::TakeOutOfService,
            "no camera matches this identity (hardware removed or manual config error)",
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
        // The recovery loop owns this camera: the target it steps toward is the
        // retrying state itself, so a camera already retrying holds and one
        // that drifted gets stepped back rather than promoted mid-recovery.
        CameraDesired::Retrying => CameraStateCurrent::Retrying,
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
        let clock = Arc::new(crate::clock::MockClock::new(1000));
        Reconciler::with_clock(Audit::with_clock(clock.clone()), clock)
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

    /// A relay that records what it was issued, through a handle the test
    /// keeps (`Arc<Mutex<..>>` so the `Box`'d relay shares the log).
    #[derive(Debug, Default)]
    struct Issued(std::sync::Arc<std::sync::Mutex<Vec<(String, CommandId)>>>);

    impl CommandRelay for Issued {
        fn issue(&mut self, camera_id: &str, command: CommandId) {
            self.0
                .lock()
                .unwrap()
                .push((camera_id.to_string(), command));
        }
    }

    #[test]
    fn commands_follow_the_action_not_the_grade() {
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);
        let mut r = reconciler();

        // Drift up to streaming authorizes the stream start.
        r.set_desired("cam", CameraDesired::Armed);
        let rem = r.reconcile(&mut camera);
        assert_eq!(rem.commands(), vec![CommandId::StartCameraStream]);

        // An unreachable camera authorizes the reconnect probe.
        let mut offline = observed("cam", CameraStateCurrent::On, 1.0);
        offline.online = false;
        let rem = r.reconcile(&mut offline);
        assert_eq!(rem.action, Action::MarkOffline);
        assert_eq!(rem.commands(), vec![CommandId::TestCameraConnection]);
    }

    #[test]
    fn a_hold_licenses_no_command_even_at_a_high_grade() {
        // Auth failure grades high (reconfigure) but holds: only an operator
        // can supply credentials, so nothing may be issued.
        let mut r = reconciler();
        r.record_error("cam", CameraError::AuthFailed);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);
        let rem = r.reconcile(&mut camera);
        assert_eq!(rem.grade, Grade::Reconfigure);
        assert!(rem.commands().is_empty(), "{rem:?}");

        // Same for a matching camera: grade None, no commands.
        let mut quiet = observed("cam", CameraStateCurrent::On, 1.0);
        quiet.desired = CameraDesired::Idle;
        let rem = r.reconcile(&mut quiet);
        assert_eq!(rem.grade, Grade::None);
        assert!(rem.commands().is_empty());
    }

    #[test]
    fn taking_a_camera_out_of_service_authorizes_disabling_it() {
        let mut r = reconciler();
        r.set_desired("cam", CameraDesired::Disabled);
        let mut camera = observed("cam", CameraStateCurrent::Streaming, 1.0);
        let rem = r.reconcile(&mut camera);
        assert_eq!(rem.action, Action::TakeOutOfService);
        assert_eq!(rem.commands(), vec![CommandId::CameraSetEnabled]);
    }

    #[test]
    fn an_installed_relay_receives_the_commands_as_they_are_graded() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut r = reconciler().with_command_relay(Box::new(Issued(std::sync::Arc::clone(&log))));
        r.set_desired("cam", CameraDesired::Armed);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);
        r.reconcile(&mut camera);

        let issued = log.lock().unwrap().clone();
        assert_eq!(
            issued,
            vec![("cam".to_string(), CommandId::StartCameraStream)]
        );
        // The issuance itself is audited, after the remediation entry.
        let audit = r.into_audit();
        let whats: Vec<&str> = audit.entries().iter().map(|e| e.what.as_str()).collect();
        assert!(
            whats.iter().any(|w| w.starts_with("issued ")),
            "issuance must be audited: {whats:?}"
        );
    }

    #[test]
    fn without_a_relay_the_reconciler_stays_report_only() {
        let mut r = reconciler();
        r.set_desired("cam", CameraDesired::Armed);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);
        let rem = r.reconcile(&mut camera);
        // The remediation still names what it would issue...
        assert_eq!(rem.commands(), vec![CommandId::StartCameraStream]);
        // ...and the audit carries no issuance entry.
        let audit = r.into_audit();
        assert!(audit
            .entries()
            .iter()
            .all(|e| !e.what.starts_with("issued ")));
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

    #[test]
    fn device_busy_grades_to_wait_and_retry() {
        let mut r = reconciler();
        r.record_error("cam", CameraError::DeviceBusy);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);

        let remediation = r.reconcile(&mut camera);
        assert_eq!(remediation.grade, Grade::Retry);
        assert_eq!(remediation.action, Action::Hold);
        assert!(remediation.reason.contains("device busy"));
    }

    #[test]
    fn media_unavailable_grades_to_defer() {
        let mut r = reconciler();
        r.record_error("cam", CameraError::MediaUnavailable);
        let mut camera = observed("cam", CameraStateCurrent::On, 1.0);

        let remediation = r.reconcile(&mut camera);
        assert_eq!(remediation.grade, Grade::Observe);
        assert_eq!(remediation.action, Action::Hold);
        assert!(remediation.reason.contains("media unavailable"));
    }

    #[test]
    fn hot_unplug_device_dropped_creates_ghost_device_audit() {
        let mut r = reconciler();
        let mut camera = observed("cam", CameraStateCurrent::Streaming, 1.0);

        let event = DomainEvent::DeviceDropped {
            camera_id: "cam".into(),
        };
        let remediation = r.handle_event(&event, &mut camera);

        assert_eq!(camera.state, CameraStateCurrent::Disconnected);
        assert!(!camera.online);
        assert_eq!(remediation.grade, Grade::Disable);

        let entries = r.audit().entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].what.contains("CameraDropped"));
        assert!(entries[0].what.contains("ghost device preserved"));
    }

    #[test]
    fn reconcile_group_profiles_applies_profile_to_all_cameras_in_group() {
        use cctv_core::{CameraId, GridLayout, RecordingContainer};

        let mut r = reconciler();
        let profile = RecordingProfile::new(
            vec![CameraId::new("cam-1"), CameraId::new("cam-2")],
            GridLayout::SplitHorizontal,
            None,
            RecordingContainer::Mp4,
        );

        r.set_group_profile("perimeter", profile.clone());

        let mut configs = vec![
            CameraConfig::new("cam-1", "onvif", "10.0.0.1").with_group("perimeter"),
            CameraConfig::new("cam-2", "onvif", "10.0.0.2").with_group("perimeter"),
            CameraConfig::new("cam-3", "onvif", "10.0.0.3").with_group("interior"),
        ];

        let updated = r.reconcile_group_profiles(&mut configs);
        assert_eq!(updated, vec!["cam-1", "cam-2"]);
        assert_eq!(configs[0].recording_profile, Some(profile.clone()));
        assert_eq!(configs[1].recording_profile, Some(profile));
        assert_eq!(configs[2].recording_profile, None);

        // Second pass is idempotent
        let updated_again = r.reconcile_group_profiles(&mut configs);
        assert!(updated_again.is_empty());
    }

    #[test]
    fn test_policy_safe_preset_emits_no_advance() {
        let mut r = reconciler().with_policy(Policy::safe());
        r.set_desired("cam-safe", CameraDesired::Armed);
        let mut camera = observed("cam-safe", CameraStateCurrent::On, 1.0);

        let rem = r.reconcile(&mut camera);
        // Candidate is Grade::Notify and Action::Advance(Streaming).
        // Under Policy::safe() (max_grade: Grade::Observe), candidate grade exceeds max_grade,
        // so it must be demoted to Action::Hold and camera state must not advance.
        assert_ne!(rem.action, Action::Advance(CameraStateCurrent::Streaming));
        assert_eq!(rem.action, Action::Hold);
        assert_eq!(camera.state, CameraStateCurrent::On);
        assert!(rem.commands().is_empty());

        let audit = r.audit().entries_for("cam-safe");
        assert_eq!(audit.len(), 1);
        assert!(audit[0].what.contains("[policy:safe]"));
        assert_eq!(audit[0].grade, Grade::Notify.as_u8());
    }

    #[test]
    fn test_policy_repair_preset_applies_advance_up_to_max_grade() {
        // Repair with max_grade: Grade::Notify allows Grade::Notify (Advance).
        let mut r = reconciler().with_policy(Policy::repair(Grade::Notify));
        r.set_desired("cam-repair", CameraDesired::Armed);
        let mut camera = observed("cam-repair", CameraStateCurrent::On, 1.0);

        let rem = r.reconcile(&mut camera);
        assert_eq!(rem.grade, Grade::Notify);
        assert_eq!(rem.action, Action::Advance(CameraStateCurrent::Streaming));
        assert_eq!(camera.state, CameraStateCurrent::Streaming);

        let audit = r.audit().entries_for("cam-repair");
        assert_eq!(audit.len(), 1);
        assert!(audit[0].what.contains("[policy:repair]"));
        assert_eq!(audit[0].grade, Grade::Notify.as_u8());

        // Now test error that yields Grade::Retry (Grade 3 > Grade 2).
        // It must be suppressed / demoted to Action::Hold.
        r.record_error("cam-repair", CameraError::Network);
        let rem2 = r.reconcile(&mut camera);
        assert_eq!(rem2.action, Action::Hold);
        assert_eq!(camera.state, CameraStateCurrent::Streaming); // Not marked offline
        let audit2 = r.audit().entries_for("cam-repair");
        assert_eq!(audit2.len(), 2);
        assert!(audit2[1].what.contains("[policy:repair]"));
        assert_eq!(audit2[1].grade, Grade::Retry.as_u8());
    }

    #[test]
    fn test_policy_scheduled_outside_window_emits_nothing() {
        use crate::policy::ScheduleWindow;

        // Window in the past: 1_000 to 2_000 ms.
        let window = ScheduleWindow::new(1_000, 2_000);
        let mut r = reconciler().with_policy(Policy::scheduled(window));
        // Ensure clock is set to something outside the window
        r.clock = std::sync::Arc::new(crate::clock::MockClock::new(500));
        r.set_desired("cam-sched", CameraDesired::Armed);
        let mut camera = observed("cam-sched", CameraStateCurrent::On, 1.0);

        let rem = r.reconcile(&mut camera);
        assert_eq!(rem.action, Action::Hold);
        assert_eq!(camera.state, CameraStateCurrent::On); // No mutation
        assert!(rem.commands().is_empty());

        let audit = r.audit().entries_for("cam-sched");
        assert_eq!(audit.len(), 1);
        assert!(audit[0].what.contains("[policy:scheduled]"));
        assert!(audit[0].what.contains("outside scheduled window"));
        assert_eq!(audit[0].grade, Grade::Notify.as_u8());

        // Now with window in the future encompassing now: 0 to u64::MAX
        let active_window = ScheduleWindow::new(0, u64::MAX);
        r.set_policy(Policy::scheduled(active_window));
        let rem2 = r.reconcile(&mut camera);
        assert_eq!(rem2.action, Action::Advance(CameraStateCurrent::Streaming));
        assert_eq!(camera.state, CameraStateCurrent::Streaming);
    }

    #[test]
    fn test_policy_dry_run_and_budget_and_cooldown() {
        let mut r = reconciler().with_policy(
            Policy::repair(Grade::Disable)
                .with_dry_run(true)
                .with_per_camera_budget(1)
                .with_cooldown_ms(100_000),
        );
        r.set_desired("cam-dry", CameraDesired::Armed);
        let mut camera = observed("cam-dry", CameraStateCurrent::On, 1.0);

        let rem = r.reconcile(&mut camera);
        assert_eq!(rem.action, Action::Advance(CameraStateCurrent::Streaming));
        // Dry-run should not mutate camera state
        assert_eq!(camera.state, CameraStateCurrent::On);

        let audit = r.audit().entries_for("cam-dry");
        assert_eq!(audit.len(), 1);
        assert!(audit[0].what.contains("[policy:repair] dry_run"));
        assert_eq!(audit[0].grade, Grade::Notify.as_u8());
    }

    #[test]
    fn test_policy_hysteresis() {
        use crate::policy::Policy;
        use crate::policy::PolicyTracker;
        use crate::reconcile::{Action, Grade, Remediation};

        let policy = Policy::repair(Grade::Disable).with_hysteresis_ms(5000);
        let mut tracker = PolicyTracker::new();

        let mut candidate = Remediation {
            camera_id: "cam-hyst".to_string(),
            grade: Grade::Reconfigure,
            action: Action::Advance(crate::reconcile::CameraStateCurrent::Streaming),
            reason: "test".to_string(),
        };

        // First tick at 1000ms. Should be suppressed by hysteresis.
        let verdict = policy.evaluate(candidate.clone(), 1000, &mut tracker);
        assert!(!verdict.is_executable());
        if let crate::policy::PolicyVerdict::Suppressed {
            remediation,
            reason,
        } = verdict
        {
            assert_eq!(remediation.action, Action::Hold);
            assert!(reason.contains("hysteresis"));
        } else {
            panic!("expected suppressed");
        }

        // Second tick at 4000ms. Still suppressed (elapsed 3000 < 5000).
        let verdict2 = policy.evaluate(candidate.clone(), 4000, &mut tracker);
        assert!(!verdict2.is_executable());

        // Third tick at 6500ms. Now authorized (elapsed 5500 >= 5000).
        let verdict3 = policy.evaluate(candidate.clone(), 6500, &mut tracker);
        assert!(verdict3.is_executable());
        if let crate::policy::PolicyVerdict::Apply(rem) = verdict3 {
            assert_eq!(
                rem.action,
                Action::Advance(crate::reconcile::CameraStateCurrent::Streaming)
            );
        } else {
            panic!("expected apply");
        }

        // If candidate becomes not actionable, first_seen_ms is cleared.
        candidate.action = Action::Hold;
        candidate.grade = Grade::None;
        let _ = policy.evaluate(candidate.clone(), 7000, &mut tracker);
        assert!(!tracker.first_seen_ms.contains_key("cam-hyst"));
    }

    #[test]
    fn test_policy_rollout_cap() {
        use crate::policy::Policy;
        use crate::policy::PolicyTracker;
        use crate::reconcile::{Action, Grade, Remediation};

        let policy = Policy::repair(Grade::Disable).with_rollout_cap(2);
        let mut tracker = PolicyTracker::new();
        tracker.begin_tick();

        let candidate1 = Remediation {
            camera_id: "cam-1".to_string(),
            grade: Grade::Reconfigure,
            action: Action::Advance(crate::reconcile::CameraStateCurrent::Streaming),
            reason: "test".to_string(),
        };
        let candidate2 = Remediation {
            camera_id: "cam-2".to_string(),
            grade: Grade::Reconfigure,
            action: Action::Advance(crate::reconcile::CameraStateCurrent::Streaming),
            reason: "test".to_string(),
        };
        let candidate3 = Remediation {
            camera_id: "cam-3".to_string(),
            grade: Grade::Reconfigure,
            action: Action::Advance(crate::reconcile::CameraStateCurrent::Streaming),
            reason: "test".to_string(),
        };

        // First two apply.
        assert!(policy
            .evaluate(candidate1, 1000, &mut tracker)
            .is_executable());
        assert!(policy
            .evaluate(candidate2, 1000, &mut tracker)
            .is_executable());

        // Third is suppressed due to rollout cap.
        let verdict3 = policy.evaluate(candidate3, 1000, &mut tracker);
        assert!(!verdict3.is_executable());
        if let crate::policy::PolicyVerdict::Suppressed { reason, .. } = verdict3 {
            assert!(reason.contains("rollout cap"));
        } else {
            panic!("expected suppressed");
        }

        // New tick clears limit.
        tracker.begin_tick();
        let candidate3_retry = Remediation {
            camera_id: "cam-3".to_string(),
            grade: Grade::Reconfigure,
            action: Action::Advance(crate::reconcile::CameraStateCurrent::Streaming),
            reason: "test".to_string(),
        };
        assert!(policy
            .evaluate(candidate3_retry, 2000, &mut tracker)
            .is_executable());
    }
}
