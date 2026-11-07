//! Live-stream health: allocation-free state machine for live-stream health.

/// Health of a live stream as reported by the watchdog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// A frame arrived inside the stall budget: the stream is live.
    Healthy,
    /// No frame arrived within the stall budget: the stream is silent.
    Stalled {
        /// When the stall state began (milliseconds on the caller's clock).
        since_ms: u64,
    },
    /// The watch has been retired (the driver exhausted its reconnect budget):
    /// a silent stream is terminal until the watch is re-armed.
    Dead {
        /// When the watch was retired.
        since_ms: u64,
    },
}

impl Health {
    /// Short label for dashboards/logs.
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Stalled { .. } => "stalled",
            Self::Dead { .. } => "dead",
        }
    }
}

/// One reported change of state: *what* changed and *when*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthChanged {
    /// The new health of the stream.
    pub health: Health,
    /// When the change happened, milliseconds on the caller's clock.
    pub since_ms: u64,
}

impl HealthChanged {
    /// Short reason for logs/dashboards.
    pub fn reason(self) -> &'static str {
        match self.health {
            Health::Healthy => "live",
            Health::Stalled { .. } => "stalled",
            Health::Dead { .. } => "dead",
        }
    }
}

/// Allocation-free watchdog: pure state machine over `(last frame, now, stall
/// budget)`.
///
/// The watchdog never sleeps and never reads a clock; it turns the stream's
/// own timestamps into a health decision, so every rule is a unit test.  The
/// caller keeps the watchdog armed for the lifetime of a stream; it is
/// re-created per stream.
///
/// # Example
///
/// ```rust,no_run
/// # use crate::health::{HealthChanged, HealthWatchdog};
/// let mut watchdog = HealthWatchdog::new();
/// // a frame at 500 ms: healthy
/// assert_eq!(watchdog.tick(500, 500, 15_000).map(|c| c.health), Some(Health::Healthy));
/// // 14 s of silence: still inside a 15 s budget
/// assert_eq!(watchdog.tick(500, 14_000, 15_000).map(|c| c.health), Some(Health::Healthy));
/// // silence crossed the budget: stuck in stalled
/// assert!(watchdog.tick(500, 16_000, 15_000).is_some());
/// assert_eq!(watchdog.health, Health::Stalled { since_ms: 1_000 });
/// ```
#[derive(Debug)]
pub struct HealthWatchdog {
    /// The health observed at this point in time.
    pub health: Health,
}

impl HealthWatchdog {
    /// Create a watch that has not seen a frame yet.
    pub fn new() -> Self {
        Self {
            health: Health::Dead { since_ms: 0 },
        }
    }

    /// Decide the current health from `(last frame, now, stall budget)`.
    ///
    /// Returns `None` when nothing changed since the last tick.
    pub fn tick(
        &mut self,
        last_frame_ms: u64,
        now_ms: u64,
        stall_ms: u64,
    ) -> Option<HealthChanged> {
        let silent = now_ms > last_frame_ms && (now_ms - last_frame_ms) > stall_ms;
        let next = if silent {
            if self.health == Health::Dead { self.health } else { Health::Stalled { since_ms: now_ms - stall_ms } }
        } else {
            Health::Healthy
        };
        if next == self.health {
            return None;
        }
        let changed = HealthChanged { health: next, since_ms: now_ms };
        self.health = next;
        Some(changed)
    }

    /// The driver arms the watch as dead once the reconnect budget is spent.
    pub fn arm_dead(&mut self, since_ms: u64) {
        self.health = Health::Dead { since_ms };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_watchdog_is_dead_until_the_first_frame() {
        let mut watchdog = HealthWatchdog::new();
        assert_eq!(watchdog.health, Health::Dead { since_ms: 0 });
    }

    #[test]
    fn first_frame_makes_it_healthy() {
        let mut watchdog = HealthWatchdog::new();
        let changed = watchdog.tick(500, 500, 15_000).expect("a change");
        assert_eq!(changed.health, Health::Healthy);
        assert_eq!(changed.since_ms, 500);
        assert_eq!(watchdog.health, Health::Healthy);
    }

    #[test]
    fn a_frame_within_the_budget_keeps_the_stream_healthy() {
        let mut watchdog = HealthWatchdog::new();
        watchdog.tick(500, 500, 15_000); // healthy again
        // 14 s of silence: still inside a 15 s budget.
        let c = watchdog.tick(500, 14_000, 15_000).expect("a change");
        assert_eq!(c.health, Health::Healthy);
        assert_eq!(watchdog.health, Health::Healthy);
    }

    #[test]
    fn silence_beyond_the_budget_marks_stalled() {
        let mut watchdog = HealthWatchdog::new();
        watchdog.tick(500, 500, 15_000); // healthy
        let stalled = watchdog
            .tick(500, 16_000, 15_000)
            .expect("a change should occur at 16 s");
        assert_eq!(stalled.health, Health::Stalled { since_ms: 1_000 });
        assert_eq!(stalled.reason(), "stalled");
    }

    #[test]
    fn a_stalled_stream_stays_stalled_until_a_frame_renews_it() {
        let mut watchdog = HealthWatchdog::new();
        watchdog.tick(500, 500, 15_000);
        watchdog.tick(500, 20_000, 15_000); // stalled
        // no frame for 30 s more: still stalled, no second change
        assert_eq!(watchdog.tick(500, 50_000, 15_000), None);
        assert_eq!(watchdog.health, Health::Stalled { since_ms: 1_000 });
    }

    #[test]
    fn a_dead_watchdog_stays_dead_while_silent() {
        let mut watchdog = HealthWatchdog::new();
        watchdog.tick(500, 500, 15_000);
        watchdog.tick(500, 20_000, 15_000); // stalled
        // the driver re-arms dead after its reconnect retries are exhausted
        watchdog.arm_dead(20_000);
        assert_eq!(watchdog.health, Health::Dead { since_ms: 20_000 });
        // a frame while it is dead is *not* a recovery: the watch is retired
        assert_eq!(watchdog.tick(500, 21_000, 15_000), None);
    }

    #[test]
    fn a_dead_watchdog_recovers_when_re_armed() {
        let mut watchdog = HealthWatchdog::new();
        watchdog.tick(500, 500, 15_000);
        watchdog.tick(500, 20_000, 15_000); // stalled
        watchdog.arm_dead(20_000);
        // a frame after the watch has been re-armed: healthy again
        assert_eq!(watchdog.tick(500, 21_000, 15_000).map(|c| c.health), Some(Health::Healthy));
        assert_eq!(watchdog.health, Health::Healthy);
    }

    #[test]
    fn no_change_is_not_reported() {
        let mut watchdog = HealthWatchdog::new();
        assert!(watchdog.tick(500, 500, 15_000).is_some());
        assert_eq!(watchdog.tick(500, 5_000, 15_000), None);
    }

    #[test]
    fn stalled_since_marks_the_start_of_the_silent_window() {
        let mut watchdog = HealthWatchdog::new();
        watchdog.tick(500, 500, 15_000);
        let changed = watchdog.tick(500, 16_000, 15_000).expect("stalled");
        // the silent window started 1 s before the budget was breached
        assert_eq!(changed.since_ms, 1_000);
    }
}
