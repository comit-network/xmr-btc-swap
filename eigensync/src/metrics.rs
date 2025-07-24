//! Metrics and observability for eigensync

#[cfg(feature = "metrics")]
use prometheus::{Counter, Histogram, Registry};
use std::time::Instant;

/// Metrics collector for eigensync operations
pub struct EigensyncMetrics {
    #[cfg(feature = "metrics")]
    registry: Registry,
    #[cfg(feature = "metrics")]
    changes_sent: Counter,
    #[cfg(feature = "metrics")]
    changes_received: Counter,
    #[cfg(feature = "metrics")]
    sync_duration: Histogram,
    #[cfg(feature = "metrics")]
    rtt_histogram: Histogram,
}

impl EigensyncMetrics {
    /// Create a new metrics collector
    pub fn new() -> Self {
        tracing::debug!("Creating eigensync metrics collector");
        
        #[cfg(feature = "metrics")]
        {
            let registry = Registry::new();
            
            let changes_sent = Counter::new(
                "eigensync_changes_sent_total",
                "Total number of changes sent"
            ).expect("Failed to create changes_sent counter");
            
            let changes_received = Counter::new(
                "eigensync_changes_received_total", 
                "Total number of changes received"
            ).expect("Failed to create changes_received counter");
            
            let sync_duration = Histogram::with_opts(
                prometheus::HistogramOpts::new(
                    "eigensync_sync_duration_seconds",
                    "Duration of sync operations in seconds"
                )
            ).expect("Failed to create sync_duration histogram");
            
            let rtt_histogram = Histogram::with_opts(
                prometheus::HistogramOpts::new(
                    "eigensync_request_rtt_seconds",
                    "Round-trip time for requests in seconds"
                )
            ).expect("Failed to create rtt histogram");
            
            registry.register(Box::new(changes_sent.clone())).unwrap();
            registry.register(Box::new(changes_received.clone())).unwrap();
            registry.register(Box::new(sync_duration.clone())).unwrap();
            registry.register(Box::new(rtt_histogram.clone())).unwrap();
            
            Self {
                registry,
                changes_sent,
                changes_received,
                sync_duration,
                rtt_histogram,
            }
        }
        
        #[cfg(not(feature = "metrics"))]
        {
            Self {}
        }
    }

    /// Record changes sent
    pub fn record_changes_sent(&self, count: u64) {
        #[cfg(feature = "metrics")]
        {
            self.changes_sent.inc_by(count as f64);
        }
        
        tracing::debug!("Recorded {} changes sent", count);
    }

    /// Record changes received
    pub fn record_changes_received(&self, count: u64) {
        #[cfg(feature = "metrics")]
        {
            self.changes_received.inc_by(count as f64);
        }
        
        tracing::debug!("Recorded {} changes received", count);
    }

    /// Record sync operation duration
    pub fn record_sync_duration(&self, duration: std::time::Duration) {
        #[cfg(feature = "metrics")]
        {
            self.sync_duration.observe(duration.as_secs_f64());
        }
        
        tracing::debug!("Recorded sync duration: {:?}", duration);
    }

    /// Record request round-trip time
    pub fn record_rtt(&self, rtt: std::time::Duration) {
        #[cfg(feature = "metrics")]
        {
            self.rtt_histogram.observe(rtt.as_secs_f64());
        }
        
        tracing::debug!("Recorded RTT: {:?}", rtt);
    }

    /// Get metrics registry (for Prometheus export)
    #[cfg(feature = "metrics")]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
}

impl Default for EigensyncMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Timer for measuring operation duration
pub struct Timer {
    start: Instant,
    label: String,
}

impl Timer {
    /// Start a new timer
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            label: label.into(),
        }
    }

    /// Stop the timer and return the elapsed duration
    pub fn stop(self) -> std::time::Duration {
        let duration = self.start.elapsed();
        tracing::debug!("Timer '{}' finished in {:?}", self.label, duration);
        duration
    }
}

/// Macro for timing operations
#[macro_export]
macro_rules! time_operation {
    ($metrics:expr, $operation:expr, $block:block) => {{
        let timer = crate::metrics::Timer::new($operation);
        let result = $block;
        let duration = timer.stop();
        $metrics.record_sync_duration(duration);
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let _metrics = EigensyncMetrics::new();
        // Metrics creation should not panic
    }

    #[test]
    fn test_timer() {
        let timer = Timer::new("test");
        std::thread::sleep(std::time::Duration::from_millis(1));
        let duration = timer.stop();
        assert!(duration.as_millis() >= 1);
    }

    #[test]
    fn test_metrics_recording() {
        let metrics = EigensyncMetrics::new();
        
        // These should not panic
        metrics.record_changes_sent(5);
        metrics.record_changes_received(3);
        metrics.record_sync_duration(std::time::Duration::from_millis(100));
        metrics.record_rtt(std::time::Duration::from_millis(50));
    }
} 