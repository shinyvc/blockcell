use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Layer 3: Session Memory
// ============================================================================

/// Layer 3 metrics - Session memory.
#[derive(Debug, Default)]
pub struct Layer3Metrics {
    /// Number of extractions.
    extraction_count: AtomicU64,
    /// Number of loads.
    load_count: AtomicU64,
    /// Total token estimate.
    total_token_estimate: AtomicU64,
    /// Current session memory size.
    current_size: AtomicU64,
    // --- 新增字段 ---
    /// Max total tokens limit (配置值, 默认 12000)
    max_total_tokens: AtomicU64,
    /// Max section length (配置值, 默认 2000)
    max_section_length: AtomicU64,
    /// Last extraction timestamp (Unix ms)
    last_extraction_timestamp: AtomicU64,
    /// Current section count
    section_count: AtomicU64,
    /// 成功提取次数
    success_count: AtomicU64,
    /// 失败提取次数
    failure_count: AtomicU64,
    /// 总 token 消耗
    total_token_cost: AtomicU64,
}

impl Layer3Metrics {
    /// Record an extraction event.
    pub fn record_extraction(&self, token_estimate: u64) {
        self.extraction_count.fetch_add(1, Ordering::Relaxed);
        self.total_token_estimate
            .fetch_add(token_estimate, Ordering::Relaxed);
        self.last_extraction_timestamp.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    /// Record a load event.
    pub fn record_load(&self, size: u64) {
        self.load_count.fetch_add(1, Ordering::Relaxed);
        self.current_size.store(size, Ordering::Relaxed);
    }

    /// Record an extraction completed event.
    pub fn record_extraction_completed(&self, success: bool, token_cost: u64, _sections: u64) {
        if success {
            self.success_count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failure_count.fetch_add(1, Ordering::Relaxed);
        }
        self.total_token_cost
            .fetch_add(token_cost, Ordering::Relaxed);
    }

    /// Record config settings for Layer 3.
    pub fn record_config(&self, max_total: u64, max_section: u64) {
        self.max_total_tokens.store(max_total, Ordering::Relaxed);
        self.max_section_length
            .store(max_section, Ordering::Relaxed);
    }

    /// Update section count.
    pub fn update_section_count(&self, count: u64) {
        self.section_count.store(count, Ordering::Relaxed);
    }

    /// Update current size (without incrementing load_count).
    /// Use this for resetting state values, not for recording load events.
    pub fn update_current_size(&self, size: u64) {
        self.current_size.store(size, Ordering::Relaxed);
    }

    /// Get the extraction count.
    pub fn extraction_count(&self) -> u64 {
        self.extraction_count.load(Ordering::Relaxed)
    }

    /// Get the load count.
    pub fn load_count(&self) -> u64 {
        self.load_count.load(Ordering::Relaxed)
    }

    /// Get the current size.
    pub fn current_size(&self) -> u64 {
        self.current_size.load(Ordering::Relaxed)
    }

    /// Get the total token estimate.
    pub fn total_token_estimate(&self) -> u64 {
        self.total_token_estimate.load(Ordering::Relaxed)
    }

    /// Get max total tokens limit.
    pub fn max_total_tokens(&self) -> u64 {
        self.max_total_tokens.load(Ordering::Relaxed)
    }

    /// Get max section length.
    pub fn max_section_length(&self) -> u64 {
        self.max_section_length.load(Ordering::Relaxed)
    }

    /// Get last extraction timestamp.
    pub fn last_extraction_timestamp(&self) -> u64 {
        self.last_extraction_timestamp.load(Ordering::Relaxed)
    }

    /// Get section count.
    pub fn section_count(&self) -> u64 {
        self.section_count.load(Ordering::Relaxed)
    }

    /// Get success count.
    pub fn success_count(&self) -> u64 {
        self.success_count.load(Ordering::Relaxed)
    }

    /// Get failure count.
    pub fn failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::Relaxed)
    }

    /// Get total token cost.
    pub fn total_token_cost(&self) -> u64 {
        self.total_token_cost.load(Ordering::Relaxed)
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.extraction_count.store(0, Ordering::Relaxed);
        self.load_count.store(0, Ordering::Relaxed);
        self.total_token_estimate.store(0, Ordering::Relaxed);
        self.current_size.store(0, Ordering::Relaxed);
        self.section_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        self.failure_count.store(0, Ordering::Relaxed);
        self.total_token_cost.store(0, Ordering::Relaxed);
        // 配置值和时间戳保留
    }
}
