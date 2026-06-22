use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Layer 7: Forked Agent
// ============================================================================

/// Layer 7 metrics - Forked agent.
#[derive(Debug, Default)]
pub struct Layer7Metrics {
    /// Number of agents spawned.
    spawned_count: AtomicU64,
    /// Number of agents completed.
    completed_count: AtomicU64,
    /// Number of agents failed.
    failed_count: AtomicU64,
    /// Number of tool denied events.
    tool_denied_count: AtomicU64,
    /// Total tokens used.
    total_tokens_used: AtomicU64,
    /// Total turns used.
    total_turns_used: AtomicU64,
    // --- 新增字段 ---
    /// Max turns per agent (配置值)
    max_turns: AtomicU64,
    /// Current active agents
    active_count: AtomicU64,
    /// Total completion time in ms
    total_completion_time_ms: AtomicU64,
    /// Cumulative cache hit rate sum (for avg calculation).
    cache_hit_rate_sum: AtomicU64,
    /// Number of cache hit rate samples.
    cache_hit_rate_samples: AtomicU64,
}

impl Layer7Metrics {
    /// Record an agent spawned event.
    pub fn record_spawned(&self) {
        self.spawned_count.fetch_add(1, Ordering::Relaxed);
        self.active_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an agent completed event.
    pub fn record_completed(&self, turns: u64, tokens: u64) {
        self.completed_count.fetch_add(1, Ordering::Relaxed);
        self.total_turns_used.fetch_add(turns, Ordering::Relaxed);
        self.total_tokens_used.fetch_add(tokens, Ordering::Relaxed);
        let _ = self
            .active_count
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| x.checked_sub(1));
    }

    /// Record an agent completed event with duration.
    pub fn record_completed_with_duration(&self, turns: u64, tokens: u64, duration_ms: u64) {
        self.record_completed_with_duration_and_rate(turns, tokens, duration_ms, 0.0);
    }

    /// Record an agent completed event with duration and cache hit rate.
    pub fn record_completed_with_duration_and_rate(
        &self,
        turns: u64,
        tokens: u64,
        duration_ms: u64,
        cache_hit_rate: f64,
    ) {
        self.completed_count.fetch_add(1, Ordering::Relaxed);
        self.total_turns_used.fetch_add(turns, Ordering::Relaxed);
        self.total_tokens_used.fetch_add(tokens, Ordering::Relaxed);
        self.total_completion_time_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        // 累加 cache_hit_rate（乘以 1000 保留精度）
        let rate_scaled = (cache_hit_rate * 1000.0) as u64;
        self.cache_hit_rate_sum
            .fetch_add(rate_scaled, Ordering::Relaxed);
        self.cache_hit_rate_samples.fetch_add(1, Ordering::Relaxed);
        let _ = self
            .active_count
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| x.checked_sub(1));
    }

    /// Record an agent failed event.
    pub fn record_failed(&self) {
        self.failed_count.fetch_add(1, Ordering::Relaxed);
        let _ = self
            .active_count
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| x.checked_sub(1));
    }

    /// Record a tool denied event.
    pub fn record_tool_denied(&self) {
        self.tool_denied_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record config settings for Layer 7.
    pub fn record_config(&self, max_turns: u64) {
        self.max_turns.store(max_turns, Ordering::Relaxed);
    }

    /// Get the spawned count.
    pub fn spawned_count(&self) -> u64 {
        self.spawned_count.load(Ordering::Relaxed)
    }

    /// Get the completed count.
    pub fn completed_count(&self) -> u64 {
        self.completed_count.load(Ordering::Relaxed)
    }

    /// Get the failed count.
    pub fn failed_count(&self) -> u64 {
        self.failed_count.load(Ordering::Relaxed)
    }

    /// Get the tool denied count.
    pub fn tool_denied_count(&self) -> u64 {
        self.tool_denied_count.load(Ordering::Relaxed)
    }

    /// Get total tokens used.
    pub fn total_tokens_used(&self) -> u64 {
        self.total_tokens_used.load(Ordering::Relaxed)
    }

    /// Get total turns used.
    pub fn total_turns_used(&self) -> u64 {
        self.total_turns_used.load(Ordering::Relaxed)
    }

    /// Get max turns per agent.
    pub fn max_turns(&self) -> u64 {
        self.max_turns.load(Ordering::Relaxed)
    }

    /// Get current active count.
    pub fn active_count(&self) -> u64 {
        self.active_count.load(Ordering::Relaxed)
    }

    /// Get total completion time in ms.
    pub fn total_completion_time_ms(&self) -> u64 {
        self.total_completion_time_ms.load(Ordering::Relaxed)
    }

    /// Get the cache hit rate sum (scaled by 1000).
    pub fn cache_hit_rate_sum(&self) -> u64 {
        self.cache_hit_rate_sum.load(Ordering::Relaxed)
    }

    /// Get the cache hit rate samples count.
    pub fn cache_hit_rate_samples(&self) -> u64 {
        self.cache_hit_rate_samples.load(Ordering::Relaxed)
    }

    /// Calculate average cache hit rate.
    pub fn avg_cache_hit_rate(&self) -> f64 {
        let sum = self.cache_hit_rate_sum.load(Ordering::Relaxed);
        let samples = self.cache_hit_rate_samples.load(Ordering::Relaxed);
        if samples > 0 {
            sum as f64 / samples as f64 / 1000.0
        } else {
            0.0
        }
    }

    /// Calculate average completion time in ms.
    pub fn avg_completion_time_ms(&self) -> f64 {
        let completed = self.completed_count.load(Ordering::Relaxed);
        let total_time = self.total_completion_time_ms.load(Ordering::Relaxed);
        if completed > 0 {
            total_time as f64 / completed as f64
        } else {
            0.0
        }
    }

    /// Calculate average tokens per agent.
    pub fn avg_tokens_per_agent(&self) -> f64 {
        let completed = self.completed_count.load(Ordering::Relaxed);
        let total_tokens = self.total_tokens_used.load(Ordering::Relaxed);
        if completed > 0 {
            total_tokens as f64 / completed as f64
        } else {
            0.0
        }
    }

    /// Calculate average turns per agent.
    pub fn avg_turns(&self) -> f64 {
        let completed = self.completed_count.load(Ordering::Relaxed);
        let total_turns = self.total_turns_used.load(Ordering::Relaxed);
        if completed > 0 {
            total_turns as f64 / completed as f64
        } else {
            0.0
        }
    }

    /// Calculate success rate.
    pub fn success_rate(&self) -> f64 {
        let completed = self.completed_count.load(Ordering::Relaxed);
        let failed = self.failed_count.load(Ordering::Relaxed);
        let total = completed + failed;
        if total > 0 {
            completed as f64 / total as f64
        } else {
            1.0 // No agents run means 100% success
        }
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.spawned_count.store(0, Ordering::Relaxed);
        self.completed_count.store(0, Ordering::Relaxed);
        self.failed_count.store(0, Ordering::Relaxed);
        self.tool_denied_count.store(0, Ordering::Relaxed);
        self.total_tokens_used.store(0, Ordering::Relaxed);
        self.total_turns_used.store(0, Ordering::Relaxed);
        self.active_count.store(0, Ordering::Relaxed);
        self.total_completion_time_ms.store(0, Ordering::Relaxed);
        self.cache_hit_rate_sum.store(0, Ordering::Relaxed);
        self.cache_hit_rate_samples.store(0, Ordering::Relaxed);
        // 配置值保留
    }
}
