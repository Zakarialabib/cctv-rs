//! Fleet reconciliation policies.
//!
//! Controls how invasively and when the fleet reconciler can remediate state differences.
//! Provides configurable constraints on maximum grade, dry-run simulation, cooldown intervals,
//! per-camera action budgets, and optional scheduled maintenance windows.

use crate::reconcile::{Action, Grade, Remediation};
use std::collections::BTreeMap;

/// A recurring or bounded time window for scheduled maintenance and reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleWindow {
    /// Start of the window in epoch milliseconds (inclusive).
    pub start_ms: u64,
    /// End of the window in epoch milliseconds (inclusive).
    pub end_ms: u64,
}

impl ScheduleWindow {
    /// Create a new maintenance window bounded by epoch millisecond timestamps.
    pub fn new(start_ms: u64, end_ms: u64) -> Self {
        Self { start_ms, end_ms }
    }

    /// Check if the given epoch millisecond timestamp falls inside the window.
    pub fn contains(&self, now_ms: u64) -> bool {
        now_ms >= self.start_ms && now_ms <= self.end_ms
    }
}

/// Graded reconciliation policy controlling how remediation decisions are authorized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Distinct identifier of this policy (e.g. "safe", "repair", "scheduled", or custom).
    pub id: String,
    /// Maximum severity grade this policy permits to be actively executed.
    pub max_grade: Grade,
    /// When true, audits the candidate remediation without applying state changes or commands.
    pub dry_run: bool,
    /// Minimum time in milliseconds that must elapse between remediations for the same camera.
    pub cooldown_ms: u64,
    /// Maximum count of active remediations allowed per camera before exhaustion.
    pub per_camera_budget: u32,
    /// Optional scheduled maintenance window. Outside this window, active remediations are suppressed.
    pub window: Option<ScheduleWindow>,
    /// Minimum time in milliseconds a state difference must persist before being actively remediated (hysteresis).
    pub hysteresis_ms: u64,
    /// Maximum number of active remediations permitted across the fleet per tick (staged rollout cap).
    pub rollout_cap: u32,
}

impl Policy {
    /// Create a custom policy.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        max_grade: Grade,
        dry_run: bool,
        cooldown_ms: u64,
        per_camera_budget: u32,
        window: Option<ScheduleWindow>,
        hysteresis_ms: u64,
        rollout_cap: u32,
    ) -> Self {
        Self {
            id: id.into(),
            max_grade,
            dry_run,
            cooldown_ms,
            per_camera_budget,
            window,
            hysteresis_ms,
            rollout_cap,
        }
    }

    /// Safe preset: report-only, allows observation and passive monitoring, emits no Advance actions.
    ///
    /// Capped at `Grade::Observe`, ensuring no state advancing or active network commands occur.
    pub fn safe() -> Self {
        Self {
            id: "safe".to_string(),
            max_grade: Grade::Observe,
            dry_run: false,
            cooldown_ms: 0,
            per_camera_budget: u32::MAX,
            window: None,
            hysteresis_ms: 0,
            rollout_cap: u32::MAX,
        }
    }

    /// Repair preset: applies active state advances and remediations up to `max_grade`.
    pub fn repair(max_grade: Grade) -> Self {
        Self {
            id: "repair".to_string(),
            max_grade,
            dry_run: false,
            cooldown_ms: 0,
            per_camera_budget: u32::MAX,
            window: None,
            hysteresis_ms: 0,
            rollout_cap: u32::MAX,
        }
    }

    /// Scheduled preset: active remediations up to `Grade::Disable` permitted only within `window`.
    pub fn scheduled(window: ScheduleWindow) -> Self {
        Self {
            id: "scheduled".to_string(),
            max_grade: Grade::Disable,
            dry_run: false,
            cooldown_ms: 0,
            per_camera_budget: u32::MAX,
            window: Some(window),
            hysteresis_ms: 0,
            rollout_cap: u32::MAX,
        }
    }

    /// Builder method to toggle dry-run simulation mode.
    #[must_use]
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Builder method to configure per-camera cooldown in milliseconds.
    #[must_use]
    pub fn with_cooldown_ms(mut self, cooldown_ms: u64) -> Self {
        self.cooldown_ms = cooldown_ms;
        self
    }

    /// Builder method to configure per-camera remediation execution budget.
    #[must_use]
    pub fn with_per_camera_budget(mut self, budget: u32) -> Self {
        self.per_camera_budget = budget;
        self
    }

    /// Builder method to configure a scheduled maintenance window.
    #[must_use]
    pub fn with_window(mut self, window: Option<ScheduleWindow>) -> Self {
        self.window = window;
        self
    }

    /// Builder method to configure hysteresis duration in milliseconds.
    #[must_use]
    pub fn with_hysteresis_ms(mut self, hysteresis_ms: u64) -> Self {
        self.hysteresis_ms = hysteresis_ms;
        self
    }

    /// Builder method to configure staged rollout fleet limit.
    #[must_use]
    pub fn with_rollout_cap(mut self, rollout_cap: u32) -> Self {
        self.rollout_cap = rollout_cap;
        self
    }
}

/// Tracks runtime enforcement state per camera under a policy (cooldowns, action counts).
#[derive(Debug, Default, Clone)]
pub struct PolicyTracker {
    /// Timestamp (ms) of the last action applied per camera.
    pub last_action_ms: BTreeMap<String, u64>,
    /// Number of actions applied per camera.
    pub action_count: BTreeMap<String, u32>,
    /// Timestamp (ms) when a remediation was first seen for a camera.
    pub first_seen_ms: BTreeMap<String, u64>,
    /// Current tick count of actions.
    pub tick_action_count: u32,
}

impl PolicyTracker {
    /// Create a new, empty policy enforcement tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset tracked stats for a specific camera.
    pub fn reset_camera(&mut self, camera_id: &str) {
        self.last_action_ms.remove(camera_id);
        self.action_count.remove(camera_id);
        self.first_seen_ms.remove(camera_id);
    }

    /// Reset all tracked stats across the fleet.
    pub fn clear(&mut self) {
        self.last_action_ms.clear();
        self.action_count.clear();
        self.first_seen_ms.clear();
        self.tick_action_count = 0;
    }

    /// Begin a new tick, resetting per-tick limit counters.
    pub fn begin_tick(&mut self) {
        self.tick_action_count = 0;
    }
}

/// Result of evaluating a candidate remediation through a policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    /// Remediation is permitted as-is.
    Apply(Remediation),
    /// Remediation was suppressed or demoted to Hold (e.g. due to max_grade, window, budget, or cooldown).
    Suppressed {
        /// The demoted or non-acting remediation.
        remediation: Remediation,
        /// Reason for policy suppression.
        reason: String,
    },
    /// Remediation was simulated in dry_run mode without applying state change or command.
    DryRun {
        /// The candidate remediation that would have been applied.
        remediation: Remediation,
    },
}

impl PolicyVerdict {
    /// Returns the effective remediation to record in the audit trail or apply.
    pub fn remediation(&self) -> &Remediation {
        match self {
            Self::Apply(rem)
            | Self::Suppressed {
                remediation: rem, ..
            }
            | Self::DryRun { remediation: rem } => rem,
        }
    }

    /// Whether this verdict permits actively mutating the camera state and issuing wire commands.
    pub fn is_executable(&self) -> bool {
        matches!(self, Self::Apply(_))
    }
}

impl Policy {
    /// Evaluate a candidate remediation against this policy at timestamp `now_ms`.
    ///
    /// Modifies `tracker` if the remediation is accepted and actively applied.
    pub fn evaluate(
        &self,
        candidate: Remediation,
        now_ms: u64,
        tracker: &mut PolicyTracker,
    ) -> PolicyVerdict {
        // If not actionable (Grade::None), allow through without state changes or tracking consumption.
        if !candidate.is_actionable() {
            tracker.first_seen_ms.remove(&candidate.camera_id);
            return PolicyVerdict::Apply(candidate);
        }

        let first_seen = *tracker
            .first_seen_ms
            .entry(candidate.camera_id.clone())
            .or_insert(now_ms);

        // 1. Hysteresis check: must wait until camera health has been flapping/deviated for long enough.
        if self.hysteresis_ms > 0 && now_ms < first_seen.saturating_add(self.hysteresis_ms) {
            let elapsed = now_ms.saturating_sub(first_seen);
            let suppressed_rem = Remediation {
                camera_id: candidate.camera_id.clone(),
                grade: candidate.grade,
                action: Action::Hold,
                reason: format!(
                    "suppressed by policy '{}': hysteresis in progress ({} ms < {} ms)",
                    self.id, elapsed, self.hysteresis_ms
                ),
            };
            return PolicyVerdict::Suppressed {
                remediation: suppressed_rem,
                reason: "hysteresis in progress".to_string(),
            };
        }

        // 2. Scheduled window check: outside the window, active remediations are suppressed.
        if let Some(ref window) = self.window {
            if !window.contains(now_ms) {
                let suppressed_rem = Remediation {
                    camera_id: candidate.camera_id.clone(),
                    grade: candidate.grade,
                    action: Action::Hold,
                    reason: format!(
                        "suppressed by policy '{}': outside scheduled window (now: {} ms, window: [{}..{}])",
                        self.id, now_ms, window.start_ms, window.end_ms
                    ),
                };
                return PolicyVerdict::Suppressed {
                    remediation: suppressed_rem,
                    reason: "outside scheduled window".to_string(),
                };
            }
        }

        // 3. Cooldown check: must respect minimum elapsed milliseconds since previous action.
        if self.cooldown_ms > 0 {
            if let Some(&last_ms) = tracker.last_action_ms.get(&candidate.camera_id) {
                if now_ms < last_ms.saturating_add(self.cooldown_ms) {
                    let elapsed = now_ms.saturating_sub(last_ms);
                    let suppressed_rem = Remediation {
                        camera_id: candidate.camera_id.clone(),
                        grade: candidate.grade,
                        action: Action::Hold,
                        reason: format!(
                            "suppressed by policy '{}': camera in cooldown ({} ms < {} ms)",
                            self.id, elapsed, self.cooldown_ms
                        ),
                    };
                    return PolicyVerdict::Suppressed {
                        remediation: suppressed_rem,
                        reason: "camera in cooldown".to_string(),
                    };
                }
            }
        }

        // 4. Per-camera budget check: cannot exceed permitted count of remediations.
        let current_count = tracker
            .action_count
            .get(&candidate.camera_id)
            .copied()
            .unwrap_or(0);
        if current_count >= self.per_camera_budget {
            let suppressed_rem = Remediation {
                camera_id: candidate.camera_id.clone(),
                grade: candidate.grade,
                action: Action::Hold,
                reason: format!(
                    "suppressed by policy '{}': camera budget exhausted ({}/{})",
                    self.id, current_count, self.per_camera_budget
                ),
            };
            return PolicyVerdict::Suppressed {
                remediation: suppressed_rem,
                reason: "per_camera_budget exhausted".to_string(),
            };
        }

        // 5. Maximum grade enforcement:
        // Safe preset (and any policy capping below candidate.grade) must emit no Advance action.
        if candidate.grade > self.max_grade {
            let suppressed_rem = Remediation {
                camera_id: candidate.camera_id.clone(),
                grade: candidate.grade,
                action: Action::Hold,
                reason: format!(
                    "suppressed by policy '{}': grade {} exceeds max_grade {}",
                    self.id, candidate.grade, self.max_grade
                ),
            };
            return PolicyVerdict::Suppressed {
                remediation: suppressed_rem,
                reason: format!(
                    "grade {} exceeds max_grade {}",
                    candidate.grade, self.max_grade
                ),
            };
        }

        // 6. Rollout cap check: cannot exceed permitted count of remediations per tick across fleet.
        if tracker.tick_action_count >= self.rollout_cap {
            let suppressed_rem = Remediation {
                camera_id: candidate.camera_id.clone(),
                grade: candidate.grade,
                action: Action::Hold,
                reason: format!(
                    "suppressed by policy '{}': staged rollout cap exhausted ({}/{})",
                    self.id, tracker.tick_action_count, self.rollout_cap
                ),
            };
            return PolicyVerdict::Suppressed {
                remediation: suppressed_rem,
                reason: "rollout cap exhausted".to_string(),
            };
        }

        // 7. Dry-run mode: do not mutate or record consumption, report DryRun verdict.
        if self.dry_run {
            return PolicyVerdict::DryRun {
                remediation: candidate,
            };
        }

        // 8. Remediation is authorized. Record action timestamp and increment budget count.
        tracker
            .last_action_ms
            .insert(candidate.camera_id.clone(), now_ms);
        tracker
            .action_count
            .insert(candidate.camera_id.clone(), current_count.saturating_add(1));
        tracker.tick_action_count = tracker.tick_action_count.saturating_add(1);

        PolicyVerdict::Apply(candidate)
    }
}
