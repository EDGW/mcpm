//! Session-wide byte rates from independent, exact FIFO time windows.

// Imports

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A Session-wide traffic snapshot. All rates are bytes per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionTrafficStats {
    /// Successfully written and flushed frame bytes across all channels.
    pub sent: TrafficRates,
    /// Successfully decoded frame bytes across all channels.
    pub received: TrafficRates,
}

/// Current window averages and their maxima since Session creation, in bytes per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrafficRates {
    /// Bytes in the last 100 ms divided by 0.1 seconds.
    pub current_100ms: f64,
    /// Highest 100 ms average since Session creation, independent of query frequency.
    pub peak_100ms: f64,
    /// Bytes in the last second divided by one second.
    pub current_1s: f64,
    /// Highest one-second average since Session creation, independent of query frequency.
    pub peak_1s: f64,
}

pub(super) struct TrafficStats {
    sent: Mutex<DirectionState>,
    received: Mutex<DirectionState>,
}

impl TrafficStats {
    pub fn new() -> Self {
        Self {
            sent: Mutex::new(DirectionState::new()),
            received: Mutex::new(DirectionState::new()),
        }
    }

    pub fn record_sent(&self, bytes: usize) {
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        sent.record(Instant::now(), bytes as u64);
    }

    pub fn record_received(&self, bytes: usize) {
        let mut received = self.received.lock().unwrap_or_else(PoisonError::into_inner);
        received.record(Instant::now(), bytes as u64);
    }

    pub fn snapshot(&self) -> SessionTrafficStats {
        // Lock before sampling time so records and snapshots remain chronological. Always
        // acquire sent before received; transport tasks only acquire their own direction.
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        let mut received = self.received.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        SessionTrafficStats {
            sent: sent.snapshot(now),
            received: received.snapshot(now),
        }
    }
}

struct DirectionState {
    short: RateQueue,
    long: RateQueue,
}

impl DirectionState {
    fn new() -> Self {
        Self {
            short: RateQueue::new(Duration::from_millis(100)),
            long: RateQueue::new(Duration::from_secs(1)),
        }
    }

    fn record(&mut self, now: Instant, bytes: u64) {
        self.short.record(now, bytes);
        self.long.record(now, bytes);
    }

    fn snapshot(&mut self, now: Instant) -> TrafficRates {
        let (current_100ms, peak_100ms) = self.short.snapshot(now);
        let (current_1s, peak_1s) = self.long.snapshot(now);
        TrafficRates {
            current_100ms,
            peak_100ms,
            current_1s,
            peak_1s,
        }
    }
}

/// Samples are chronological; each window owns its samples, running sum, and lifetime peak.
struct RateQueue {
    expire: Duration,
    samples: VecDeque<Sample>,
    total_bytes: u64,
    peak_bytes: u64,
}

impl RateQueue {
    fn new(expire: Duration) -> Self {
        Self {
            expire,
            samples: VecDeque::new(),
            total_bytes: 0,
            peak_bytes: 0,
        }
    }

    fn record(&mut self, now: Instant, bytes: u64) {
        self.shrink(now);
        self.samples.push_back(Sample { at: now, bytes });
        self.total_bytes += bytes;
        self.peak_bytes = self.peak_bytes.max(self.total_bytes);
    }

    fn snapshot(&mut self, now: Instant) -> (f64, f64) {
        self.shrink(now);
        self.peak_bytes = self.peak_bytes.max(self.total_bytes);
        let seconds = self.expire.as_secs_f64();
        (
            self.total_bytes as f64 / seconds,
            self.peak_bytes as f64 / seconds,
        )
    }

    fn shrink(&mut self, now: Instant) {
        while self
            .samples
            .front()
            .is_some_and(|sample| now.duration_since(sample.at) > self.expire)
        {
            let sample = self.samples.pop_front().expect("front sample exists");
            self.total_bytes -= sample.bytes;
        }
    }
}

struct Sample {
    at: Instant,
    bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_boundary_is_retained_then_expires_without_clearing_peak() {
        let start = Instant::now();
        let expire = Duration::from_millis(100);
        let mut queue = RateQueue::new(expire);
        assert_eq!(queue.snapshot(start), (0.0, 0.0));
        queue.record(start, 20);
        queue.record(start + expire, 30);
        assert_eq!(queue.snapshot(start + expire), (500.0, 500.0));
        assert_eq!(
            queue.snapshot(start + expire + Duration::from_nanos(1)),
            (300.0, 500.0)
        );
        assert_eq!(queue.snapshot(start + expire * 3), (0.0, 500.0));
    }

    #[test]
    fn record_expires_old_bytes_and_captures_peaks_without_queries() {
        let start = Instant::now();
        let mut queue = RateQueue::new(Duration::from_secs(1));
        queue.record(start, 100);
        queue.record(start + Duration::from_millis(500), 200);
        queue.record(start + Duration::from_millis(1001), 50);
        assert_eq!(
            queue.snapshot(start + Duration::from_millis(1001)),
            (250.0, 300.0)
        );
        assert_eq!(queue.snapshot(start + Duration::from_secs(3)), (0.0, 300.0));
    }

    #[test]
    fn windows_expire_independently_and_keep_distinct_lifetime_peaks() {
        let start = Instant::now();
        let mut direction = DirectionState::new();
        direction.record(start, 100);
        direction.record(start + Duration::from_millis(200), 200);
        let rates = direction.snapshot(start + Duration::from_millis(301));
        assert_eq!(rates.current_100ms, 0.0);
        assert_eq!(rates.peak_100ms, 2000.0);
        assert_eq!(rates.current_1s, 300.0);
        assert_eq!(rates.peak_1s, 300.0);
        let idle = direction.snapshot(start + Duration::from_secs(2));
        assert_eq!(idle.current_100ms, 0.0);
        assert_eq!(idle.current_1s, 0.0);
        assert_eq!(idle.peak_100ms, 2000.0);
        assert_eq!(idle.peak_1s, 300.0);
    }
}
