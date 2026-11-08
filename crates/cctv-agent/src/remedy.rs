//! Device remediation and clock healing.
//!
//! Clock skew between the camera and host causes TLS handshake failures,
//! WS-Security `UsernameToken` nonce/timestamp rejections, and misaligned
//! recording indexes. If `observed_clock_offset_ms.abs() > 60_000`, the agent
//! emits `RemedyStep::SyncClockToUtc`, invokes `set_system_date_and_time`,
//! and resets `observed_clock_offset_ms = 0`.

use std::time::SystemTime;

use async_trait::async_trait;
use cctv_core::{CameraError, ImperativeCameraCommand};
use cctv_driver::onvif::build_set_system_date_and_time_soap;
use serde::{Deserialize, Serialize};

/// Maximum permissible clock drift between camera and host before healing is triggered (1 minute).
pub const MAX_CLOCK_OFFSET_MS: i64 = 60_000;

/// Remediation steps that can be emitted to correct device degradation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemedyStep {
    /// Synchronise device system clock to UTC.
    SyncClockToUtc,
}

impl RemedyStep {
    /// Convert remedy step into the canonical `ImperativeCameraCommand`.
    pub fn to_imperative_command(self) -> ImperativeCameraCommand {
        match self {
            RemedyStep::SyncClockToUtc => ImperativeCameraCommand::SyncClockToUtc,
        }
    }

    /// Build the ONVIF SOAP envelope payload for this remedy step if applicable.
    pub fn build_soap_body(self, now: SystemTime) -> Option<String> {
        match self {
            RemedyStep::SyncClockToUtc => {
                let ms = now
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                Some(build_set_system_date_and_time_soap(ms))
            }
        }
    }
}

/// Device interface for managing system clock and date/time.
#[async_trait]
pub trait ClockSyncDevice: Send + Sync {
    /// Update the camera's system date and time to the given `SystemTime` (UTC).
    async fn set_system_date_and_time(&mut self, date_time: SystemTime) -> Result<(), CameraError>;
}

/// Clock healing manager monitoring clock drift and applying remediation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClockHealing {
    /// Observed clock offset in milliseconds (device time - host time).
    pub observed_clock_offset_ms: i64,
}

impl ClockHealing {
    /// Create a new `ClockHealing` tracker with the specified initial clock offset.
    pub fn new(observed_clock_offset_ms: i64) -> Self {
        Self {
            observed_clock_offset_ms,
        }
    }

    /// Update observed clock offset.
    pub fn update_offset(&mut self, offset_ms: i64) {
        self.observed_clock_offset_ms = offset_ms;
    }

    /// Inspect clock offset and apply healing remediation if threshold is exceeded.
    ///
    /// If `observed_clock_offset_ms.abs() > 60_000`, emits `Some(RemedyStep::SyncClockToUtc)`,
    /// calls `device.set_system_date_and_time(SystemTime::now())`, and resets
    /// `observed_clock_offset_ms = 0`.
    pub async fn check_and_heal<D: ClockSyncDevice>(
        &mut self,
        device: &mut D,
    ) -> Result<Option<RemedyStep>, CameraError> {
        if self.observed_clock_offset_ms.abs() > MAX_CLOCK_OFFSET_MS {
            let now = SystemTime::now();
            device.set_system_date_and_time(now).await?;
            self.observed_clock_offset_ms = 0;
            Ok(Some(RemedyStep::SyncClockToUtc))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockClockDevice {
        last_sync: Option<SystemTime>,
        calls: usize,
    }

    #[async_trait]
    impl ClockSyncDevice for MockClockDevice {
        async fn set_system_date_and_time(
            &mut self,
            date_time: SystemTime,
        ) -> Result<(), CameraError> {
            self.last_sync = Some(date_time);
            self.calls += 1;
            Ok(())
        }
    }

    #[tokio::test]
    async fn offset_within_threshold_is_not_healed() {
        let mut device = MockClockDevice::default();
        let mut healing = ClockHealing::new(60_000);

        let step = healing.check_and_heal(&mut device).await.unwrap();
        assert_eq!(step, None);
        assert_eq!(healing.observed_clock_offset_ms, 60_000);
        assert_eq!(device.calls, 0);

        healing.update_offset(-60_000);
        let step = healing.check_and_heal(&mut device).await.unwrap();
        assert_eq!(step, None);
        assert_eq!(healing.observed_clock_offset_ms, -60_000);
        assert_eq!(device.calls, 0);
    }

    #[tokio::test]
    async fn positive_offset_exceeding_threshold_triggers_clock_sync() {
        let mut device = MockClockDevice::default();
        let mut healing = ClockHealing::new(60_001);

        let step = healing.check_and_heal(&mut device).await.unwrap();
        assert_eq!(step, Some(RemedyStep::SyncClockToUtc));
        assert_eq!(device.calls, 1);
        assert!(device.last_sync.is_some());
        assert_eq!(healing.observed_clock_offset_ms, 0);
    }

    #[tokio::test]
    async fn negative_offset_exceeding_threshold_triggers_clock_sync() {
        let mut device = MockClockDevice::default();
        let mut healing = ClockHealing::new(-120_000);

        let step = healing.check_and_heal(&mut device).await.unwrap();
        assert_eq!(step, Some(RemedyStep::SyncClockToUtc));
        assert_eq!(device.calls, 1);
        assert!(device.last_sync.is_some());
        assert_eq!(healing.observed_clock_offset_ms, 0);
    }
}
