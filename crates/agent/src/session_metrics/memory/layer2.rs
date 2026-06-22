use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Layer 2: Micro Compact
// ============================================================================

/// Layer 2 metrics - Micro compact (time-based cleanup).
#[derive(Debug, Default)]
pub struct Layer2Metrics {
    /// Number of time-based triggers.
    trigger_count: AtomicU64,
    /// Number of items cleared.
    cleared_count: AtomicU64,
    /// Number of items kept.
    kept_count: AtomicU64,
    // --- 新增字段 ---
    /// Gap threshold in minutes (配置值)
    gap_threshold_minutes: AtomicU64,
    /// Keep recent count (配置值)
    keep_recent: AtomicU64,
    /// Last trigger timestamp (Unix ms)
    last_trigger_timestamp: AtomicU64,
    /// Number of evaluation checks performed.
    evaluated_count: AtomicU64,
    /// Number of times evaluation did not trigger.
    not_triggered_count: AtomicU64,
}

impl Layer2Metrics {
    /// Record a trigger event.
    pub fn record_trigger(&self) {
        self.trigger_count.fetch_add(1, Ordering::Relaxed);
        self.last_trigger_timestamp.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    /// Record an evaluation event.
    pub fn record_evaluated(&self, triggered: bool) {
        self.evaluated_count.fetch_add(1, Ordering::Relaxed);
        if !triggered {
            self.not_triggered_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record cleared items.
    pub fn record_cleared(&self, cleared: u64, kept: u64) {
        self.cleared_count.fetch_add(cleared, Ordering::Relaxed);
        self.kept_count.fetch_add(kept, Ordering::Relaxed);
    }

    /// Record config settings for Layer 2.
    pub fn record_config(&self, gap_minutes: u64, keep_recent: u64) {
        self.gap_threshold_minutes
            .store(gap_minutes, Ordering::Relaxed);
        self.keep_recent.store(keep_recent, Ordering::Relaxed);
    }

    /// Get the trigger count.
    pub fn trigger_count(&self) -> u64 {
        self.trigger_count.load(Ordering::Relaxed)
    }

    /// Get the cleared count.
    pub fn cleared_count(&self) -> u64 {
        self.cleared_count.load(Ordering::Relaxed)
    }

    /// Get the kept count.
    pub fn kept_count(&self) -> u64 {
        self.kept_count.load(Ordering::Relaxed)
    }

    /// Get gap threshold in minutes.
    pub fn gap_threshold_minutes(&self) -> u64 {
        self.gap_threshold_minutes.load(Ordering::Relaxed)
    }

    /// Get keep recent count.
    pub fn keep_recent(&self) -> u64 {
        self.keep_recent.load(Ordering::Relaxed)
    }

    /// Get last trigger timestamp (Unix ms).
    pub fn last_trigger_timestamp(&self) -> u64 {
        self.last_trigger_timestamp.load(Ordering::Relaxed)
    }

    /// Get the evaluated count.
    pub fn evaluated_count(&self) -> u64 {
        self.evaluated_count.load(Ordering::Relaxed)
    }

    /// Get the not triggered count.
    pub fn not_triggered_count(&self) -> u64 {
        self.not_triggered_count.load(Ordering::Relaxed)
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.trigger_count.store(0, Ordering::Relaxed);
        self.cleared_count.store(0, Ordering::Relaxed);
        self.kept_count.store(0, Ordering::Relaxed);
        self.evaluated_count.store(0, Ordering::Relaxed);
        self.not_triggered_count.store(0, Ordering::Relaxed);
        // 新增字段不重置（配置值和时间戳保留）
    }
}
