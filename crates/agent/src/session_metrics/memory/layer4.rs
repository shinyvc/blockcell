use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Layer 4: Full Compact
// ============================================================================

/// Layer 4 metrics - Full compact (LLM-based compression).
#[derive(Debug, Default)]
pub struct Layer4Metrics {
    /// Total compact count.
    compact_count: AtomicU64,
    /// Auto compact count.
    auto_compact_count: AtomicU64,
    /// Manual compact count.
    manual_compact_count: AtomicU64,
    /// Failed compact count.
    compact_failed_count: AtomicU64,
    /// Consecutive failures (for circuit breaker).
    consecutive_failures: AtomicU64,
    /// Total pre-compact tokens.
    total_pre_compact_tokens: AtomicU64,
    /// Total post-compact tokens.
    total_post_compact_tokens: AtomicU64,
    /// Total cache read tokens.
    total_cache_read_tokens: AtomicU64,
    /// Total cache creation tokens.
    total_cache_creation_tokens: AtomicU64,
    // --- 新增字段 ---
    /// Token budget (配置值, 默认 100000)
    token_budget: AtomicU64,
    /// Threshold ratio (存储为整数, 0.8 -> 800)
    threshold_ratio: AtomicU64,
    /// Current tokens in use (实时状态)
    current_tokens: AtomicU64,
    /// Last compact timestamp (Unix ms)
    last_compact_timestamp: AtomicU64,
    /// Total recovery budget (文件 50K + 技能 25K + Session 12K = 87K)
    total_recovery_budget: AtomicU64,
    /// Number of retries.
    retry_count: AtomicU64,
    /// Number of cache break events.
    cache_break_count: AtomicU64,
    /// Total recovery tokens used.
    total_recovery_tokens: AtomicU64,
}

impl Layer4Metrics {
    /// Record a successful compact.
    pub fn record_compact_success(
        &self,
        pre_tokens: u64,
        post_tokens: u64,
        cache_read: u64,
        cache_creation: u64,
    ) {
        self.record_compact_success_with_type(
            pre_tokens,
            post_tokens,
            cache_read,
            cache_creation,
            true,
        );
    }

    /// Record a successful compact with type distinction.
    pub fn record_compact_success_with_type(
        &self,
        pre_tokens: u64,
        post_tokens: u64,
        cache_read: u64,
        cache_creation: u64,
        is_auto: bool,
    ) {
        self.compact_count.fetch_add(1, Ordering::Relaxed);
        if is_auto {
            self.auto_compact_count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.manual_compact_count.fetch_add(1, Ordering::Relaxed);
        }
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.total_pre_compact_tokens
            .fetch_add(pre_tokens, Ordering::Relaxed);
        self.total_post_compact_tokens
            .fetch_add(post_tokens, Ordering::Relaxed);
        self.total_cache_read_tokens
            .fetch_add(cache_read, Ordering::Relaxed);
        self.total_cache_creation_tokens
            .fetch_add(cache_creation, Ordering::Relaxed);
        // 更新时间戳
        self.last_compact_timestamp.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    /// Record a failed compact.
    pub fn record_compact_failure(&self) {
        self.compact_failed_count.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a retry event.
    pub fn record_retry(&self) {
        self.retry_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a cache break event.
    pub fn record_cache_break(&self) {
        self.cache_break_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record recovery tokens used.
    pub fn record_recovery_tokens(&self, tokens: u64) {
        self.total_recovery_tokens
            .fetch_add(tokens, Ordering::Relaxed);
    }

    /// Record config settings for Layer 4.
    pub fn record_config(&self, budget: u64, threshold: f64, recovery_budget: u64) {
        self.token_budget.store(budget, Ordering::Relaxed);
        self.threshold_ratio
            .store((threshold * 1000.0) as u64, Ordering::Relaxed);
        self.total_recovery_budget
            .store(recovery_budget, Ordering::Relaxed);
    }

    /// Update current token usage (实时状态).
    pub fn update_token_usage(&self, current: u64) {
        self.current_tokens.store(current, Ordering::Relaxed);
    }

    /// Get the compact count.
    pub fn compact_count(&self) -> u64 {
        self.compact_count.load(Ordering::Relaxed)
    }

    /// Get the failed count.
    pub fn compact_failed_count(&self) -> u64 {
        self.compact_failed_count.load(Ordering::Relaxed)
    }

    /// Get the consecutive failures.
    pub fn consecutive_failures(&self) -> u64 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }

    /// Calculate average compression ratio.
    pub fn average_compression_ratio(&self) -> f64 {
        let pre = self.total_pre_compact_tokens.load(Ordering::Relaxed);
        let post = self.total_post_compact_tokens.load(Ordering::Relaxed);
        if pre > 0 {
            1.0 - (post as f64 / pre as f64)
        } else {
            0.0
        }
    }

    /// Calculate cache hit rate.
    pub fn cache_hit_rate(&self) -> f64 {
        let read = self.total_cache_read_tokens.load(Ordering::Relaxed);
        let creation = self.total_cache_creation_tokens.load(Ordering::Relaxed);
        let total = read + creation;
        if total > 0 {
            read as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Get auto compact count.
    pub fn auto_compact_count(&self) -> u64 {
        self.auto_compact_count.load(Ordering::Relaxed)
    }

    /// Get manual compact count.
    pub fn manual_compact_count(&self) -> u64 {
        self.manual_compact_count.load(Ordering::Relaxed)
    }

    /// Get total pre-compact tokens.
    pub fn total_pre_compact_tokens(&self) -> u64 {
        self.total_pre_compact_tokens.load(Ordering::Relaxed)
    }

    /// Get total post-compact tokens.
    pub fn total_post_compact_tokens(&self) -> u64 {
        self.total_post_compact_tokens.load(Ordering::Relaxed)
    }

    /// Get token budget.
    pub fn token_budget(&self) -> u64 {
        self.token_budget.load(Ordering::Relaxed)
    }

    /// Get threshold ratio (as float, 800 -> 0.8).
    pub fn threshold_ratio(&self) -> f64 {
        self.threshold_ratio.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Get threshold tokens (budget * threshold_ratio).
    pub fn threshold_tokens(&self) -> u64 {
        let budget = self.token_budget.load(Ordering::Relaxed);
        let ratio = self.threshold_ratio.load(Ordering::Relaxed);
        (budget * ratio) / 1000
    }

    /// Get current tokens.
    pub fn current_tokens(&self) -> u64 {
        self.current_tokens.load(Ordering::Relaxed)
    }

    /// Get remaining tokens (budget - current).
    pub fn remaining_tokens(&self) -> u64 {
        let budget = self.token_budget.load(Ordering::Relaxed);
        let current = self.current_tokens.load(Ordering::Relaxed);
        budget.saturating_sub(current)
    }

    /// Get usage percentage.
    pub fn usage_percentage(&self) -> f64 {
        let budget = self.token_budget.load(Ordering::Relaxed);
        let current = self.current_tokens.load(Ordering::Relaxed);
        if budget > 0 {
            (current as f64 / budget as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Get last compact timestamp.
    pub fn last_compact_timestamp(&self) -> u64 {
        self.last_compact_timestamp.load(Ordering::Relaxed)
    }

    /// Get total recovery budget.
    pub fn total_recovery_budget(&self) -> u64 {
        self.total_recovery_budget.load(Ordering::Relaxed)
    }

    /// Get the retry count.
    pub fn retry_count(&self) -> u64 {
        self.retry_count.load(Ordering::Relaxed)
    }

    /// Get the cache break count.
    pub fn cache_break_count(&self) -> u64 {
        self.cache_break_count.load(Ordering::Relaxed)
    }

    /// Get total recovery tokens.
    pub fn total_recovery_tokens(&self) -> u64 {
        self.total_recovery_tokens.load(Ordering::Relaxed)
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.compact_count.store(0, Ordering::Relaxed);
        self.auto_compact_count.store(0, Ordering::Relaxed);
        self.manual_compact_count.store(0, Ordering::Relaxed);
        self.compact_failed_count.store(0, Ordering::Relaxed);
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.total_pre_compact_tokens.store(0, Ordering::Relaxed);
        self.total_post_compact_tokens.store(0, Ordering::Relaxed);
        self.total_cache_read_tokens.store(0, Ordering::Relaxed);
        self.total_cache_creation_tokens.store(0, Ordering::Relaxed);
        // 新增字段：配置值保留，状态值重置
        self.current_tokens.store(0, Ordering::Relaxed);
        self.retry_count.store(0, Ordering::Relaxed);
        self.cache_break_count.store(0, Ordering::Relaxed);
        self.total_recovery_tokens.store(0, Ordering::Relaxed);
    }
}
