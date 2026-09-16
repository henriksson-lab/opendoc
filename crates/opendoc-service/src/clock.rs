//! Time, injectable.
//!
//! Session expiry and presence liveness are the only two things the service
//! reads a clock for, and both are much easier to test when the test owns the
//! clock than when it owns a `sleep`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Clock {
    source: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl Clock {
    pub fn system() -> Self {
        Self {
            source: Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as u64)
                    .unwrap_or(0)
            }),
        }
    }

    pub fn from_fn(source: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self {
            source: Arc::new(source),
        }
    }

    pub fn now_ms(&self) -> u64 {
        (self.source)()
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::system()
    }
}

impl std::fmt::Debug for Clock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Clock")
            .field("now_ms", &self.now_ms())
            .finish()
    }
}

/// A clock the test moves by hand.
#[derive(Clone, Debug, Default)]
pub struct ManualClock {
    now_ms: Arc<AtomicU64>,
}

impl ManualClock {
    pub fn new(now_ms: u64) -> Self {
        Self {
            now_ms: Arc::new(AtomicU64::new(now_ms)),
        }
    }

    pub fn advance_ms(&self, delta: u64) {
        self.now_ms.fetch_add(delta, Ordering::SeqCst);
    }

    pub fn set_ms(&self, now_ms: u64) {
        self.now_ms.store(now_ms, Ordering::SeqCst);
    }

    pub fn clock(&self) -> Clock {
        let now_ms = Arc::clone(&self.now_ms);
        Clock::from_fn(move || now_ms.load(Ordering::SeqCst))
    }
}
