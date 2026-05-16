//! Learning dedup — prevents duplicate learning within a time window
//!
//! 提供两种 API：
//! - `is_duplicate()`: 原子检查+写入（兼容旧调用）
//! - `contains_recent()` + `record()`: 拆分版，允许调用方在确认 review 会启动后再 record，
//!   避免 throttle 失败时 dedup 已写入导致 key 被提前消耗。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Deduplication for learning operations
///
/// Tracks recently triggered learning keys and prevents the same
/// learning from firing again within the dedup window.
pub struct LearningDedup {
    recent: Mutex<HashMap<String, Instant>>,
    dedup_window_secs: u64,
}

impl LearningDedup {
    pub fn new(dedup_window_secs: u64) -> Self {
        Self {
            recent: Mutex::new(HashMap::new()),
            dedup_window_secs,
        }
    }

    /// Check if a learning key has been seen recently
    ///
    /// Returns `true` if the key is a duplicate (seen within the window).
    /// Returns `false` and records the key if it's new or expired.
    pub fn is_duplicate(&self, key: &str) -> bool {
        // When dedup_window_secs is 0, deduplication is effectively disabled.
        // Return early to avoid wasteful inserts into the map.
        if self.dedup_window_secs == 0 {
            return false;
        }

        let mut recent = self.recent.lock().unwrap_or_else(|e| {
            tracing::warn!("LearningDedup Mutex poisoned, recovering");
            e.into_inner()
        });

        // Prune expired entries
        let window = self.dedup_window_secs;
        recent.retain(|_, instant| instant.elapsed().as_secs() < window);

        if let Some(seen_at) = recent.get(key) {
            if seen_at.elapsed().as_secs() < window {
                return true;
            }
        }

        recent.insert(key.to_string(), Instant::now());
        false
    }

    /// 只读检查：key 是否在去重窗口内已存在
    ///
    /// 与 `is_duplicate()` 不同，此方法不会写入去重表。
    /// 供调用方在 throttle/dedup 检查阶段使用，避免 throttle 失败时
    /// dedup 已写入导致 key 被提前消耗。
    pub fn contains_recent(&self, key: &str) -> bool {
        if self.dedup_window_secs == 0 {
            return false;
        }

        let recent = self.recent.lock().unwrap_or_else(|e| {
            tracing::warn!("LearningDedup Mutex poisoned, recovering");
            e.into_inner()
        });

        let window = self.dedup_window_secs;
        if let Some(seen_at) = recent.get(key) {
            seen_at.elapsed().as_secs() < window
        } else {
            false
        }
    }

    /// 写入去重记录：标记 key 已在当前时间触发
    ///
    /// 供调用方在确认 review 会启动后调用，与 `contains_recent()` 配合使用。
    pub fn record(&self, key: &str) {
        if self.dedup_window_secs == 0 {
            return;
        }

        let mut recent = self.recent.lock().unwrap_or_else(|e| {
            tracing::warn!("LearningDedup Mutex poisoned, recovering");
            e.into_inner()
        });

        recent.insert(key.to_string(), Instant::now());
    }

    /// Clear all dedup entries
    pub fn clear(&self) {
        let mut recent = self.recent.lock().unwrap_or_else(|e| {
            tracing::warn!("LearningDedup Mutex poisoned during clear");
            e.into_inner()
        });
        recent.clear();
    }

    /// Get the number of tracked entries
    pub fn len(&self) -> usize {
        let recent = self.recent.lock().unwrap_or_else(|e| {
            tracing::warn!("LearningDedup Mutex poisoned during len");
            e.into_inner()
        });
        recent.len()
    }

    /// Check if there are no tracked entries
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::fmt::Debug for LearningDedup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LearningDedup")
            .field("dedup_window_secs", &self.dedup_window_secs)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_key_not_duplicate() {
        let dedup = LearningDedup::new(600);
        assert!(!dedup.is_duplicate("test-key"));
    }

    #[test]
    fn test_same_key_is_duplicate() {
        let dedup = LearningDedup::new(600);
        assert!(!dedup.is_duplicate("test-key"));
        assert!(dedup.is_duplicate("test-key"));
    }

    #[test]
    fn test_different_keys_not_duplicate() {
        let dedup = LearningDedup::new(600);
        assert!(!dedup.is_duplicate("key-1"));
        assert!(!dedup.is_duplicate("key-2"));
    }

    #[test]
    fn test_clear_resets_state() {
        let dedup = LearningDedup::new(600);
        assert!(!dedup.is_duplicate("test-key"));
        dedup.clear();
        assert!(!dedup.is_duplicate("test-key"));
    }

    #[test]
    fn test_zero_window_allows_all() {
        let dedup = LearningDedup::new(0);
        assert!(!dedup.is_duplicate("test-key"));
        // With 0 window, the entry is immediately expired on next check
        assert!(!dedup.is_duplicate("test-key"));
    }
}
