use std::sync::atomic::{AtomicU64, Ordering};

// ============================================================================
// Layer 6: Auto Dream
// ============================================================================

/// Layer 6 metrics - Auto dream (consolidation).
#[derive(Debug, Default)]
pub struct Layer6Metrics {
    /// Number of dream runs.
    dream_count: AtomicU64,
    /// Memories created.
    memories_created: AtomicU64,
    /// Memories updated.
    memories_updated: AtomicU64,
    /// Memories deleted.
    memories_deleted: AtomicU64,
    /// Sessions pruned.
    sessions_pruned: AtomicU64,
    // --- 新增字段 ---
    /// Dream interval hours (配置值)
    dream_interval_hours: AtomicU64,
    /// Last dream timestamp (Unix ms)
    last_dream_timestamp: AtomicU64,
    /// Sessions processed
    sessions_processed: AtomicU64,
    /// Number of gate checks passed.
    gate_passed_count: AtomicU64,
    /// Number of phases completed.
    phase_completed_count: AtomicU64,
    /// Number of dream failures.
    failure_count: AtomicU64,
}

impl Layer6Metrics {
    /// Record a dream started event.
    pub fn record_dream_started(&self) {
        self.dream_count.fetch_add(1, Ordering::Relaxed);
        self.last_dream_timestamp.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    /// Record a gate passed event.
    pub fn record_gate_passed(&self) {
        self.gate_passed_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a phase completed event.
    pub fn record_phase_completed(&self) {
        self.phase_completed_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a dream failure event.
    pub fn record_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a dream finished event.
    pub fn record_dream_finished(
        &self,
        created: u64,
        updated: u64,
        deleted: u64,
        pruned: u64,
        sessions: u64,
    ) {
        self.memories_created.fetch_add(created, Ordering::Relaxed);
        self.memories_updated.fetch_add(updated, Ordering::Relaxed);
        self.memories_deleted.fetch_add(deleted, Ordering::Relaxed);
        self.sessions_pruned.fetch_add(pruned, Ordering::Relaxed);
        self.sessions_processed
            .fetch_add(sessions, Ordering::Relaxed);
    }

    /// Record config settings for Layer 6.
    pub fn record_config(&self, interval_hours: u64) {
        self.dream_interval_hours
            .store(interval_hours, Ordering::Relaxed);
    }

    /// Get the dream count.
    pub fn dream_count(&self) -> u64 {
        self.dream_count.load(Ordering::Relaxed)
    }

    /// Get memories created count.
    pub fn memories_created(&self) -> u64 {
        self.memories_created.load(Ordering::Relaxed)
    }

    /// Get memories updated count.
    pub fn memories_updated(&self) -> u64 {
        self.memories_updated.load(Ordering::Relaxed)
    }

    /// Get memories deleted count.
    pub fn memories_deleted(&self) -> u64 {
        self.memories_deleted.load(Ordering::Relaxed)
    }

    /// Get sessions pruned count.
    pub fn sessions_pruned(&self) -> u64 {
        self.sessions_pruned.load(Ordering::Relaxed)
    }

    /// Get dream interval hours.
    pub fn dream_interval_hours(&self) -> u64 {
        self.dream_interval_hours.load(Ordering::Relaxed)
    }

    /// Get last dream timestamp.
    pub fn last_dream_timestamp(&self) -> u64 {
        self.last_dream_timestamp.load(Ordering::Relaxed)
    }

    /// Get sessions processed.
    pub fn sessions_processed(&self) -> u64 {
        self.sessions_processed.load(Ordering::Relaxed)
    }

    /// Get the gate passed count.
    pub fn gate_passed_count(&self) -> u64 {
        self.gate_passed_count.load(Ordering::Relaxed)
    }

    /// Get the phase completed count.
    pub fn phase_completed_count(&self) -> u64 {
        self.phase_completed_count.load(Ordering::Relaxed)
    }

    /// Get the dream failure count.
    pub fn failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::Relaxed)
    }

    /// Calculate consolidation rate.
    pub fn consolidation_rate(&self) -> f64 {
        let processed = self.sessions_processed.load(Ordering::Relaxed);
        let created = self.memories_created.load(Ordering::Relaxed);
        let updated = self.memories_updated.load(Ordering::Relaxed);
        if processed > 0 {
            (created + updated) as f64 / processed as f64
        } else {
            0.0
        }
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.dream_count.store(0, Ordering::Relaxed);
        self.memories_created.store(0, Ordering::Relaxed);
        self.memories_updated.store(0, Ordering::Relaxed);
        self.memories_deleted.store(0, Ordering::Relaxed);
        self.sessions_pruned.store(0, Ordering::Relaxed);
        self.sessions_processed.store(0, Ordering::Relaxed);
        self.gate_passed_count.store(0, Ordering::Relaxed);
        self.phase_completed_count.store(0, Ordering::Relaxed);
        self.failure_count.store(0, Ordering::Relaxed);
        // 配置值和时间戳保留
    }
}
