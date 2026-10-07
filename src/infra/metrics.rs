//! Runtime request counters exposed through the Prometheus `/metrics` endpoint.
//!
//! Only cheap atomic counters live here; token totals and live gauges are
//! derived from the database and `AppState` at scrape time so the hot path
//! stays free of extra work.

use std::sync::atomic::{AtomicU64, Ordering};

/// Atomic counters for chat request outcomes since process start.
#[derive(Default)]
pub struct Metrics {
    requests_success: AtomicU64,
    requests_error: AtomicU64,
    requests_rate_limited: AtomicU64,
}

/// Point-in-time copy of every counter, safe to format outside the hot path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub requests_success: u64,
    pub requests_error: u64,
    pub requests_rate_limited: u64,
}

impl MetricsSnapshot {
    /// Total requests handled, across every outcome.
    pub fn requests_total(&self) -> u64 {
        self.requests_success + self.requests_error + self.requests_rate_limited
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// A request completed and was returned to the client.
    pub fn record_success(&self) {
        self.requests_success.fetch_add(1, Ordering::Relaxed);
    }

    /// A request failed with a fatal error before reaching the client.
    pub fn record_error(&self) {
        self.requests_error.fetch_add(1, Ordering::Relaxed);
    }

    /// A request ended with a 429 because every token was exhausted.
    pub fn record_rate_limited(&self) {
        self.requests_rate_limited.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_success: self.requests_success.load(Ordering::Relaxed),
            requests_error: self.requests_error.load(Ordering::Relaxed),
            requests_rate_limited: self.requests_rate_limited.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_accumulate_and_sum() {
        let metrics = Metrics::new();
        metrics.record_success();
        metrics.record_success();
        metrics.record_error();
        metrics.record_rate_limited();

        let snap = metrics.snapshot();
        assert_eq!(snap.requests_success, 2);
        assert_eq!(snap.requests_error, 1);
        assert_eq!(snap.requests_rate_limited, 1);
        assert_eq!(snap.requests_total(), 4);
    }
}
