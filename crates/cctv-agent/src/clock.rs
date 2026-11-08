//! Clock trait and implementations for deterministic time.

/// A clock that can be used to get the current time in milliseconds.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Get the current time in milliseconds.
    fn now_ms(&self) -> u64;
}

/// A clock that uses the system time.
#[derive(Debug, Default, Clone)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// A mock clock for tests.
#[derive(Debug)]
pub struct MockClock {
    /// The current time in milliseconds.
    pub current_ms: std::sync::atomic::AtomicU64,
}

impl MockClock {
    /// Create a new mock clock with the given initial time.
    pub fn new(initial_ms: u64) -> Self {
        Self {
            current_ms: std::sync::atomic::AtomicU64::new(initial_ms),
        }
    }

    /// Advance the clock by the given number of milliseconds.
    pub fn advance(&self, ms: u64) {
        self.current_ms
            .fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
    }

    /// Set the clock to the given number of milliseconds.
    pub fn set(&self, ms: u64) {
        self.current_ms
            .store(ms, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Clock for MockClock {
    fn now_ms(&self) -> u64 {
        self.current_ms.load(std::sync::atomic::Ordering::SeqCst)
    }
}
