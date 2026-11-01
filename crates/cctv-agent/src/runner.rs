//! Asynchronous runner event loop for agent tasks.
//!
//! Runs the `tokio::select!` loop monitoring incoming PTZ pulses and watchdog
//! expiration timers. When a deadline expires without renewal or explicit stop,
//! immediately dispatches `onvif.ptz_stop()` and records a `PtzWatchdogTripped`
//! event into the JSONL audit log.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cctv_core::CameraError;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;

use crate::ptz_watchdog::{PtzPulse, PtzStop, PtzWatchdog};
use crate::session_budget::CameraId;

/// An append-only JSONL audit log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonlAuditEntry {
    /// Timestamp in Unix milliseconds.
    pub timestamp_ms: u64,
    /// Camera identifier.
    pub camera_id: CameraId,
    /// Event name or tag.
    pub event: String,
    /// Informative details about the event.
    pub details: String,
}

impl JsonlAuditEntry {
    /// Create a new audit entry.
    pub fn new(camera_id: CameraId, event: impl Into<String>, details: impl Into<String>) -> Self {
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            timestamp_ms,
            camera_id,
            event: event.into(),
            details: details.into(),
        }
    }

    /// Serialise entry to a JSON line.
    pub fn to_jsonl(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// JSONL audit logger trait for recording agent operational events.
#[async_trait]
pub trait JsonlAuditLog: Send + Sync {
    /// Append a log entry.
    async fn append(&self, entry: JsonlAuditEntry);
}

/// In-memory implementation of JSONL audit log for testing and local operation.
#[derive(Debug, Default)]
pub struct InMemoryJsonlAuditLog {
    entries: Mutex<Vec<JsonlAuditEntry>>,
}

impl InMemoryJsonlAuditLog {
    /// Create a new empty audit log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a snapshot of all logged entries.
    pub async fn snapshot(&self) -> Vec<JsonlAuditEntry> {
        self.entries.lock().await.clone()
    }
}

#[async_trait]
impl JsonlAuditLog for InMemoryJsonlAuditLog {
    async fn append(&self, entry: JsonlAuditEntry) {
        self.entries.lock().await.push(entry);
    }
}

/// Dispatcher interface for sending PTZ stop commands to cameras.
#[async_trait]
pub trait OnvifPtzDispatcher: Send + Sync {
    /// Issue emergency PTZ stop to a camera.
    async fn ptz_stop(&self, camera_id: &CameraId) -> Result<(), CameraError>;
}

/// Input messages sent to the runner's PTZ watchdog handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerCommand {
    /// Heartbeat pulse keeping PTZ moving.
    Pulse(PtzPulse),
    /// Explicit stop canceling PTZ watchdog.
    Stop(PtzStop),
    /// Shutdown command to terminate the runner.
    Shutdown,
}

/// Runner executing the PTZ watchdog loop.
pub struct AgentRunner {
    watchdog: PtzWatchdog,
    dispatcher: Arc<dyn OnvifPtzDispatcher>,
    audit_log: Arc<dyn JsonlAuditLog>,
}

impl AgentRunner {
    /// Create a new runner.
    pub fn new(dispatcher: Arc<dyn OnvifPtzDispatcher>, audit_log: Arc<dyn JsonlAuditLog>) -> Self {
        Self {
            watchdog: PtzWatchdog::new(),
            dispatcher,
            audit_log,
        }
    }

    /// Run the main event loop until receiving `RunnerCommand::Shutdown` or channel close.
    pub async fn run(&mut self, mut rx: mpsc::Receiver<RunnerCommand>) {
        loop {
            let next_deadline = self.watchdog.next_deadline();
            let sleep_fut = async {
                match next_deadline {
                    Some(deadline) => {
                        let now = std::time::Instant::now();
                        if deadline > now {
                            sleep(deadline - now).await;
                        }
                    }
                    None => {
                        // Inactivity sleep when no active deadlines are armed
                        sleep(Duration::from_secs(3600)).await;
                    }
                }
            };

            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        Some(RunnerCommand::Pulse(pulse)) => {
                            self.watchdog.pulse(&pulse);
                        }
                        Some(RunnerCommand::Stop(stop)) => {
                            self.watchdog.stop(&stop);
                        }
                        Some(RunnerCommand::Shutdown) | None => {
                            break;
                        }
                    }
                }
                _ = sleep_fut => {
                    let expired = self.watchdog.check_expired();
                    for camera_id in expired {
                        // Immediately dispatch onvif.ptz_stop()
                        let _ = self.dispatcher.ptz_stop(&camera_id).await;

                        // Record PtzWatchdogTripped in JSONL audit log
                        let entry = JsonlAuditEntry::new(
                            camera_id,
                            "PtzWatchdogTripped",
                            "PTZ pulse deadline expired without renewal; stop dispatched"
                        );
                        self.audit_log.append(entry).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockDispatcher {
        stops: Mutex<Vec<CameraId>>,
        call_count: AtomicUsize,
    }

    impl MockDispatcher {
        fn new() -> Self {
            Self {
                stops: Mutex::new(Vec::new()),
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl OnvifPtzDispatcher for MockDispatcher {
        async fn ptz_stop(&self, camera_id: &CameraId) -> Result<(), CameraError> {
            self.stops.lock().await.push(camera_id.clone());
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn deadline_expiration_dispatches_ptz_stop_and_logs_audit() {
        let dispatcher = Arc::new(MockDispatcher::new());
        let audit = Arc::new(InMemoryJsonlAuditLog::new());
        let mut runner = AgentRunner::new(
            Arc::clone(&dispatcher) as Arc<dyn OnvifPtzDispatcher>,
            Arc::clone(&audit) as Arc<dyn JsonlAuditLog>,
        );

        let (tx, rx) = mpsc::channel(10);

        let runner_handle = tokio::spawn(async move {
            runner.run(rx).await;
        });

        let cam = CameraId::from("cam-ptz-tripped");
        // Pulse with short TTL (20ms)
        tx.send(RunnerCommand::Pulse(PtzPulse::new(cam.clone(), 20)))
            .await
            .unwrap();

        // Allow deadline to elapse
        tokio::time::sleep(Duration::from_millis(60)).await;

        let stops = dispatcher.stops.lock().await.clone();
        assert_eq!(stops, vec![cam.clone()]);

        let entries = audit.snapshot().await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].camera_id, cam);
        assert_eq!(entries[0].event, "PtzWatchdogTripped");
        assert!(entries[0].details.contains("PTZ pulse deadline expired"));

        tx.send(RunnerCommand::Shutdown).await.unwrap();
        runner_handle.await.unwrap();
    }

    #[tokio::test]
    async fn renewal_prevents_watchdog_trip() {
        let dispatcher = Arc::new(MockDispatcher::new());
        let audit = Arc::new(InMemoryJsonlAuditLog::new());
        let mut runner = AgentRunner::new(
            Arc::clone(&dispatcher) as Arc<dyn OnvifPtzDispatcher>,
            Arc::clone(&audit) as Arc<dyn JsonlAuditLog>,
        );

        let (tx, rx) = mpsc::channel(10);
        let runner_handle = tokio::spawn(async move {
            runner.run(rx).await;
        });

        let cam = CameraId::from("cam-ptz-renewed");
        // Pulse with 50ms TTL
        tx.send(RunnerCommand::Pulse(PtzPulse::new(cam.clone(), 50)))
            .await
            .unwrap();

        // Renew at 25ms with 100ms TTL
        tokio::time::sleep(Duration::from_millis(25)).await;
        tx.send(RunnerCommand::Pulse(PtzPulse::new(cam.clone(), 100)))
            .await
            .unwrap();

        // Sleep 40ms (now total 65ms > initial 50ms, but within renewed deadline)
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(dispatcher.call_count.load(Ordering::SeqCst), 0);

        tx.send(RunnerCommand::Stop(PtzStop::new(cam.clone())))
            .await
            .unwrap();
        // Give time to process stop
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(dispatcher.call_count.load(Ordering::SeqCst), 0);

        tx.send(RunnerCommand::Shutdown).await.unwrap();
        runner_handle.await.unwrap();
    }
}
