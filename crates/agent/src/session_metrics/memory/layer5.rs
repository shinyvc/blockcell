use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Layer 5: Memory Extraction
// ============================================================================

/// Layer 5 metrics - Memory extraction (auto-memory).
#[derive(Debug, Default)]
pub struct Layer5Metrics {
    /// Number of extractions.
    extraction_count: AtomicU64,
    /// User memories count.
    user_memories: AtomicU64,
    /// Project memories count.
    project_memories: AtomicU64,
    /// Feedback memories count.
    feedback_memories: AtomicU64,
    /// Reference memories count.
    reference_memories: AtomicU64,
    /// Total bytes written.
    total_bytes_written: AtomicU64,
    // --- 新增字段 ---
    /// Min messages for extraction (配置值, 默认 10)
    min_messages: AtomicU64,
    /// Cooldown messages (配置值, 默认 5)
    cooldown_messages: AtomicU64,
    /// Max file tokens (配置值, 默认 4000)
    max_file_tokens: AtomicU64,
    /// Last extraction timestamp (Unix ms)
    last_extraction_timestamp: AtomicU64,
    /// User memory bytes
    user_bytes: AtomicU64,
    /// Project memory bytes
    project_bytes: AtomicU64,
    /// Feedback memory bytes
    feedback_bytes: AtomicU64,
    /// Reference memory bytes
    reference_bytes: AtomicU64,
    /// Injection count (separate from memory counts to avoid inflation).
    injection_count: AtomicU64,
    /// Number of extractions started.
    extraction_started_count: AtomicU64,
    /// Number of cursor updates.
    cursor_updated_count: AtomicU64,
    /// Number of extraction failures.
    /// TODO: 当前未接入业务路径 — 提取失败通过 `cb.record_failure()` 熔断器记录，
    /// 此计数器预留用于细粒度失败分类统计。
    extraction_failure_count: AtomicU64,
}

impl Layer5Metrics {
    /// Record a memory written event.
    pub fn record_memory_written(&self, memory_type: &str, content_len: u64) {
        self.extraction_count.fetch_add(1, Ordering::Relaxed);
        self.total_bytes_written
            .fetch_add(content_len, Ordering::Relaxed);
        self.last_extraction_timestamp.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            Ordering::Relaxed,
        );

        match memory_type {
            "user" => {
                self.user_memories.fetch_add(1, Ordering::Relaxed);
                self.user_bytes.fetch_add(content_len, Ordering::Relaxed);
            }
            "project" => {
                self.project_memories.fetch_add(1, Ordering::Relaxed);
                self.project_bytes.fetch_add(content_len, Ordering::Relaxed);
            }
            "feedback" => {
                self.feedback_memories.fetch_add(1, Ordering::Relaxed);
                self.feedback_bytes
                    .fetch_add(content_len, Ordering::Relaxed);
            }
            "reference" => {
                self.reference_memories.fetch_add(1, Ordering::Relaxed);
                self.reference_bytes
                    .fetch_add(content_len, Ordering::Relaxed);
            }
            _ => {}
        };
    }

    /// Record a memory injection event (increments only injection_count,
    /// not the individual memory counters, to avoid count inflation).
    pub fn record_injection(&self, _user: u64, _project: u64, _feedback: u64, _reference: u64) {
        self.injection_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an extraction started event.
    pub fn record_extraction_started(&self) {
        self.extraction_started_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record a cursor updated event.
    pub fn record_cursor_updated(&self) {
        self.cursor_updated_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an extraction failure event.
    pub fn record_extraction_failure(&self) {
        self.extraction_failure_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record config settings for Layer 5.
    pub fn record_config(&self, min_msg: u64, cooldown: u64, max_file: u64) {
        self.min_messages.store(min_msg, Ordering::Relaxed);
        self.cooldown_messages.store(cooldown, Ordering::Relaxed);
        self.max_file_tokens.store(max_file, Ordering::Relaxed);
    }

    /// Get the extraction count.
    pub fn extraction_count(&self) -> u64 {
        self.extraction_count.load(Ordering::Relaxed)
    }

    /// Get the total bytes written.
    pub fn total_bytes_written(&self) -> u64 {
        self.total_bytes_written.load(Ordering::Relaxed)
    }

    /// Get user memories count.
    pub fn user_memories(&self) -> u64 {
        self.user_memories.load(Ordering::Relaxed)
    }

    /// Get project memories count.
    pub fn project_memories(&self) -> u64 {
        self.project_memories.load(Ordering::Relaxed)
    }

    /// Get feedback memories count.
    pub fn feedback_memories(&self) -> u64 {
        self.feedback_memories.load(Ordering::Relaxed)
    }

    /// Get reference memories count.
    pub fn reference_memories(&self) -> u64 {
        self.reference_memories.load(Ordering::Relaxed)
    }

    /// Get min messages for extraction.
    pub fn min_messages(&self) -> u64 {
        self.min_messages.load(Ordering::Relaxed)
    }

    /// Get cooldown messages.
    pub fn cooldown_messages(&self) -> u64 {
        self.cooldown_messages.load(Ordering::Relaxed)
    }

    /// Get max file tokens.
    pub fn max_file_tokens(&self) -> u64 {
        self.max_file_tokens.load(Ordering::Relaxed)
    }

    /// Get last extraction timestamp.
    pub fn last_extraction_timestamp(&self) -> u64 {
        self.last_extraction_timestamp.load(Ordering::Relaxed)
    }

    /// Get user memory bytes.
    pub fn user_bytes(&self) -> u64 {
        self.user_bytes.load(Ordering::Relaxed)
    }

    /// Get project memory bytes.
    pub fn project_bytes(&self) -> u64 {
        self.project_bytes.load(Ordering::Relaxed)
    }

    /// Get feedback memory bytes.
    pub fn feedback_bytes(&self) -> u64 {
        self.feedback_bytes.load(Ordering::Relaxed)
    }

    /// Get reference memory bytes.
    pub fn reference_bytes(&self) -> u64 {
        self.reference_bytes.load(Ordering::Relaxed)
    }

    /// Get injection count (number of batch injection events).
    pub fn injection_count(&self) -> u64 {
        self.injection_count.load(Ordering::Relaxed)
    }

    /// Get the extraction started count.
    pub fn extraction_started_count(&self) -> u64 {
        self.extraction_started_count.load(Ordering::Relaxed)
    }

    /// Get the cursor updated count.
    pub fn cursor_updated_count(&self) -> u64 {
        self.cursor_updated_count.load(Ordering::Relaxed)
    }

    /// Get the extraction failure count.
    pub fn extraction_failure_count(&self) -> u64 {
        self.extraction_failure_count.load(Ordering::Relaxed)
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.extraction_count.store(0, Ordering::Relaxed);
        self.user_memories.store(0, Ordering::Relaxed);
        self.project_memories.store(0, Ordering::Relaxed);
        self.feedback_memories.store(0, Ordering::Relaxed);
        self.reference_memories.store(0, Ordering::Relaxed);
        self.total_bytes_written.store(0, Ordering::Relaxed);
        // 新增字段
        self.user_bytes.store(0, Ordering::Relaxed);
        self.project_bytes.store(0, Ordering::Relaxed);
        self.feedback_bytes.store(0, Ordering::Relaxed);
        self.reference_bytes.store(0, Ordering::Relaxed);
        self.injection_count.store(0, Ordering::Relaxed);
        self.extraction_started_count.store(0, Ordering::Relaxed);
        self.cursor_updated_count.store(0, Ordering::Relaxed);
        self.extraction_failure_count.store(0, Ordering::Relaxed);
        // 配置值保留
    }
}
