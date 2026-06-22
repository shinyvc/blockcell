//! Memory Metrics - Global metrics for the 7-layer memory system.
//!
//! Uses lock-free atomic counters for high-performance concurrent access.
//!
//! ## Data Persistence
//!
//! **Important:** All metrics are stored in-memory only and reset on application restart.
//! This is by design - metrics are meant for real-time monitoring and debugging during
//! a session. For persistent metrics or historical analysis, consider:
//!
//! - Exporting metrics to external monitoring systems (Prometheus, Grafana, etc.)
//! - Using `/session_metrics --json` to capture snapshots for external storage
//! - Implementing a custom metrics exporter using the `MemoryMetrics::snapshot()` method

use std::sync::OnceLock;

/// Global memory system metrics instance.
///
/// Note: Metrics are in-memory only and reset on application restart.
/// For persistent metrics, consider exporting to external monitoring systems.
pub static MEMORY_METRICS: OnceLock<MemoryMetrics> = OnceLock::new();

/// Get the global memory metrics instance.
pub fn get_memory_metrics() -> &'static MemoryMetrics {
    MEMORY_METRICS.get_or_init(MemoryMetrics::default)
}

/// Global memory system metrics with lock-free counters.
#[derive(Debug, Default)]
pub struct MemoryMetrics {
    pub layer1: Layer1Metrics,
    pub layer2: Layer2Metrics,
    pub layer3: Layer3Metrics,
    pub layer4: Layer4Metrics,
    pub layer5: Layer5Metrics,
    pub layer6: Layer6Metrics,
    pub layer7: Layer7Metrics,
}

impl MemoryMetrics {
    /// Reset all layer metrics to zero.
    pub fn reset(&self) {
        self.layer1.reset();
        self.layer2.reset();
        self.layer3.reset();
        self.layer4.reset();
        self.layer5.reset();
        self.layer6.reset();
        self.layer7.reset();
    }
}

mod layer1;
mod layer2;
mod layer3;
mod layer4;
mod layer5;
mod layer6;
mod layer7;

pub use layer1::Layer1Metrics;
pub use layer2::Layer2Metrics;
pub use layer3::Layer3Metrics;
pub use layer4::Layer4Metrics;
pub use layer5::Layer5Metrics;
pub use layer6::Layer6Metrics;
pub use layer7::Layer7Metrics;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer1_metrics() {
        let metrics = Layer1Metrics::default();
        metrics.record_persisted(1000, 100);
        metrics.record_persisted(2000, 200);
        metrics.record_budget_exceeded();

        assert_eq!(metrics.persisted_count(), 2);
        assert_eq!(metrics.total_original_size(), 3000);
        assert_eq!(metrics.total_preview_size(), 300);
        assert_eq!(metrics.budget_exceeded_count(), 1);
        assert!((metrics.average_compression() - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_layer1_config() {
        let metrics = Layer1Metrics::default();
        metrics.record_config(50, 500);

        assert_eq!(metrics.max_tool_results(), 50);
        assert_eq!(metrics.preview_size_limit(), 500);

        // 测试 update_stored_count 和 increment_stored_count
        metrics.update_stored_count(10);
        assert_eq!(metrics.current_stored_results(), 10);

        metrics.increment_stored_count();
        assert_eq!(metrics.current_stored_results(), 11);
    }

    #[test]
    fn test_layer4_metrics() {
        let metrics = Layer4Metrics::default();
        metrics.record_compact_success(10000, 3000, 8000, 2000);
        metrics.record_compact_success(20000, 5000, 15000, 5000);
        metrics.record_compact_failure();

        assert_eq!(metrics.compact_count(), 2);
        assert_eq!(metrics.compact_failed_count(), 1);
        assert_eq!(metrics.consecutive_failures(), 1);

        // Compression ratio: 1 - (3000 + 5000) / (10000 + 20000) = 1 - 0.267 = 0.733
        assert!((metrics.average_compression_ratio() - 0.733).abs() < 0.01);

        // Cache hit rate: (8000 + 15000) / (8000 + 2000 + 15000 + 5000) = 23000 / 30000 = 0.767
        assert!((metrics.cache_hit_rate() - 0.767).abs() < 0.01);
    }

    #[test]
    fn test_layer4_config() {
        let metrics = Layer4Metrics::default();
        metrics.record_config(100000, 0.8, 50000);

        assert_eq!(metrics.token_budget(), 100000);
        assert!((metrics.threshold_ratio() - 0.8).abs() < 0.001);
        assert_eq!(metrics.threshold_tokens(), 80000); // 100000 * 0.8

        // 测试 update_token_usage
        metrics.update_token_usage(60000);
        assert_eq!(metrics.current_tokens(), 60000);
        assert_eq!(metrics.remaining_tokens(), 40000); // 100000 - 60000
        assert!((metrics.usage_percentage() - 60.0).abs() < 0.01);
    }

    #[test]
    fn test_layer2_config() {
        let metrics = Layer2Metrics::default();
        metrics.record_config(30, 10);

        assert_eq!(metrics.gap_threshold_minutes(), 30);
        assert_eq!(metrics.keep_recent(), 10);
    }

    #[test]
    fn test_layer3_config() {
        let metrics = Layer3Metrics::default();
        metrics.record_config(50000, 2000);

        assert_eq!(metrics.max_total_tokens(), 50000);
        assert_eq!(metrics.max_section_length(), 2000);

        // 测试 update_section_count
        metrics.update_section_count(5);
        assert_eq!(metrics.section_count(), 5);
    }

    #[test]
    fn test_layer5_config() {
        let metrics = Layer5Metrics::default();
        metrics.record_config(50, 10, 100000);

        assert_eq!(metrics.min_messages(), 50);
        assert_eq!(metrics.cooldown_messages(), 10);
        assert_eq!(metrics.max_file_tokens(), 100000);
    }

    #[test]
    fn test_layer6_config() {
        let metrics = Layer6Metrics::default();
        metrics.record_config(4);

        assert_eq!(metrics.dream_interval_hours(), 4);
    }

    #[test]
    fn test_layer7_config() {
        let metrics = Layer7Metrics::default();
        metrics.record_config(50);

        assert_eq!(metrics.max_turns(), 50);
    }

    #[test]
    fn test_layer7_statistics() {
        let metrics = Layer7Metrics::default();

        // 测试无 Agent 时的默认值
        assert!((metrics.success_rate() - 1.0).abs() < 0.001);
        assert!((metrics.avg_completion_time_ms() - 0.0).abs() < 0.001);
        assert!((metrics.avg_tokens_per_agent() - 0.0).abs() < 0.001);
        assert!((metrics.avg_turns() - 0.0).abs() < 0.001);

        // 添加一些数据
        // record_completed_with_duration 参数顺序: (turns, tokens, duration_ms)
        metrics.record_spawned();
        metrics.record_completed_with_duration(10, 1000, 500); // 10 turns, 1000 tokens, 500ms
        metrics.record_spawned();
        metrics.record_completed_with_duration(15, 2000, 800); // 15 turns, 2000 tokens, 800ms
        metrics.record_spawned();
        metrics.record_failed();

        // 计算统计数据
        assert_eq!(metrics.spawned_count(), 3);
        assert_eq!(metrics.completed_count(), 2);
        assert_eq!(metrics.failed_count(), 1);

        // success_rate: 2 / (2 + 1) = 0.667
        assert!((metrics.success_rate() - 0.667).abs() < 0.01);

        // avg_completion_time_ms: (500 + 800) / 2 = 650
        assert!((metrics.avg_completion_time_ms() - 650.0).abs() < 0.01);

        // avg_tokens_per_agent: (1000 + 2000) / 2 = 1500
        assert!((metrics.avg_tokens_per_agent() - 1500.0).abs() < 0.01);

        // avg_turns: (10 + 15) / 2 = 12.5
        assert!((metrics.avg_turns() - 12.5).abs() < 0.01);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let metrics = Arc::new(MemoryMetrics::default());

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let m = Arc::clone(&metrics);
                thread::spawn(move || {
                    m.layer1.record_persisted(i, i / 10);
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let total: u64 = (0..10).sum();
        assert_eq!(metrics.layer1.persisted_count(), 10);
        assert_eq!(metrics.layer1.total_original_size(), total);
    }
}
