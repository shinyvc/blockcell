use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Layer 1: Tool Result Storage
// ============================================================================

/// Layer 1 metrics - Tool result storage.
#[derive(Debug, Default)]
pub struct Layer1Metrics {
    /// Number of tool results persisted.
    persisted_count: AtomicU64,
    /// Total original size in bytes.
    total_original_size: AtomicU64,
    /// Total preview size in bytes.
    total_preview_size: AtomicU64,
    /// Number of budget exceeded events.
    budget_exceeded_count: AtomicU64,
    // --- 新增字段 ---
    /// Max tool results per message (配置值)
    max_tool_results: AtomicU64,
    /// Preview size limit in bytes (配置值)
    preview_size_limit: AtomicU64,
    /// Current stored results count (实时状态)
    current_stored_results: AtomicU64,
    /// Number of previews generated.
    preview_generated_count: AtomicU64,
    /// Number of replacements frozen.
    replacement_frozen_count: AtomicU64,
}

impl Layer1Metrics {
    /// Record a tool result persisted.
    pub fn record_persisted(&self, original_size: u64, preview_size: u64) {
        self.persisted_count.fetch_add(1, Ordering::Relaxed);
        self.total_original_size
            .fetch_add(original_size, Ordering::Relaxed);
        self.total_preview_size
            .fetch_add(preview_size, Ordering::Relaxed);
    }

    /// Record a tool result persisted with additional metadata fields。
    pub fn record_persisted_with_fields(
        &self,
        original_size: u64,
        preview_size: u64,
        _filepath: &str,
        _session_key: &str,
        _truncated: bool,
    ) {
        self.total_original_size
            .fetch_add(original_size, Ordering::Relaxed);
        self.total_preview_size
            .fetch_add(preview_size, Ordering::Relaxed);
        self.persisted_count.fetch_add(1, Ordering::Relaxed);
        self.current_stored_results.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a budget exceeded event.
    pub fn record_budget_exceeded(&self) {
        self.budget_exceeded_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a preview generated event.
    pub fn record_preview_generated(&self) {
        self.preview_generated_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a replacement frozen event.
    pub fn record_replacement_frozen(&self) {
        self.replacement_frozen_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Get the number of persisted tool results.
    pub fn persisted_count(&self) -> u64 {
        self.persisted_count.load(Ordering::Relaxed)
    }

    /// Get the total original size.
    pub fn total_original_size(&self) -> u64 {
        self.total_original_size.load(Ordering::Relaxed)
    }

    /// Get the total preview size.
    pub fn total_preview_size(&self) -> u64 {
        self.total_preview_size.load(Ordering::Relaxed)
    }

    /// Get the budget exceeded count.
    pub fn budget_exceeded_count(&self) -> u64 {
        self.budget_exceeded_count.load(Ordering::Relaxed)
    }

    /// Get the preview generated count.
    pub fn preview_generated_count(&self) -> u64 {
        self.preview_generated_count.load(Ordering::Relaxed)
    }

    /// Get the replacement frozen count.
    pub fn replacement_frozen_count(&self) -> u64 {
        self.replacement_frozen_count.load(Ordering::Relaxed)
    }

    // --- 新增方法 ---
    /// Record config settings for Layer 1.
    pub fn record_config(&self, max_results: u64, preview_limit: u64) {
        self.max_tool_results.store(max_results, Ordering::Relaxed);
        self.preview_size_limit
            .store(preview_limit, Ordering::Relaxed);
    }

    /// Update current stored results count.
    pub fn update_stored_count(&self, count: u64) {
        self.current_stored_results.store(count, Ordering::Relaxed);
    }

    /// Increment stored results count by 1.
    pub fn increment_stored_count(&self) {
        self.current_stored_results.fetch_add(1, Ordering::Relaxed);
    }

    /// 清理后扣减已存储结果计数。
    ///
    /// 使用 saturating CAS/fetch_update 防止下溢：
    /// 进程重启或 `/session-metrics --reset` 后内存计数器回到 0，
    /// 但磁盘上的 `.tool_results` 仍可能被下一次 cleanup 删除，
    /// 此时直接 `fetch_sub` 会将 0 下溢成接近 u64::MAX。
    pub fn decrement_stored_count(&self, count: u64) {
        let _ = self.current_stored_results.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| Some(current.saturating_sub(count)),
        );
    }

    /// Get max tool results limit.
    pub fn max_tool_results(&self) -> u64 {
        self.max_tool_results.load(Ordering::Relaxed)
    }

    /// Get preview size limit.
    pub fn preview_size_limit(&self) -> u64 {
        self.preview_size_limit.load(Ordering::Relaxed)
    }

    /// Get current stored results count.
    pub fn current_stored_results(&self) -> u64 {
        self.current_stored_results.load(Ordering::Relaxed)
    }

    /// Calculate average compression ratio.
    pub fn average_compression(&self) -> f64 {
        let orig = self.total_original_size.load(Ordering::Relaxed);
        let prev = self.total_preview_size.load(Ordering::Relaxed);
        if orig > 0 {
            1.0 - (prev as f64 / orig as f64)
        } else {
            0.0
        }
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.persisted_count.store(0, Ordering::Relaxed);
        self.total_original_size.store(0, Ordering::Relaxed);
        self.total_preview_size.store(0, Ordering::Relaxed);
        self.budget_exceeded_count.store(0, Ordering::Relaxed);
        self.preview_generated_count.store(0, Ordering::Relaxed);
        self.replacement_frozen_count.store(0, Ordering::Relaxed);
        // 新增字段不重置（配置值保留）
        self.current_stored_results.store(0, Ordering::Relaxed);
    }
}
