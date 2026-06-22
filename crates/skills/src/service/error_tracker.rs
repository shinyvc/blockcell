//! 技能错误追踪器。
//!
//! 记录每个技能在时间窗口内的错误次数与冷却期，
//! 决定何时触发自进化。从 `service.rs` 抽出以便独立维护与单测。

use std::collections::HashMap;

use crate::evolution::TriggerReason;

/// 错误追踪器：记录每个技能的错误次数和时间窗口
#[derive(Debug, Clone)]
pub(super) struct ErrorTracker {
    /// skill_name -> (错误时间戳列表, 已触发进化的时间戳)
    pub(super) errors: HashMap<String, (Vec<i64>, Option<i64>)>,
    /// 触发进化所需的连续错误次数
    threshold: u32,
    /// 错误统计的时间窗口（分钟）
    window_minutes: u32,
    /// 回滚冷却期：skill_name -> 冷却结束时间戳
    /// 在冷却期内不会触发新的进化，避免“进化→回滚→再进化”死循环
    cooldowns: HashMap<String, i64>,
    /// 冷却期时长（分钟），默认 60 分钟
    pub(super) cooldown_minutes: u32,
}

/// ErrorTracker 内部返回
pub(super) struct TrackResult {
    pub(super) count: u32,
    pub(super) is_first: bool,
    pub(super) trigger: Option<TriggerReason>,
}

impl ErrorTracker {
    pub(super) fn new(threshold: u32, window_minutes: u32, cooldown_minutes: u32) -> Self {
        Self {
            errors: HashMap::new(),
            threshold,
            window_minutes,
            cooldowns: HashMap::new(),
            cooldown_minutes,
        }
    }

    /// 记录一次错误，返回计数信息和是否触发进化
    pub(super) fn record_error(&mut self, skill_name: &str) -> TrackResult {
        let now = chrono::Utc::now().timestamp();
        let cutoff = now - (self.window_minutes as i64 * 60);

        let entry = self
            .errors
            .entry(skill_name.to_string())
            .or_insert((Vec::new(), None));
        let (timestamps, triggered_at) = entry;

        let was_empty = timestamps.is_empty();
        timestamps.push(now);

        // 清理过期的错误记录
        timestamps.retain(|&t| t > cutoff);

        // 如果已触发的进化也过期了，清除标记
        if let Some(trigger_time) = *triggered_at {
            if trigger_time <= cutoff {
                *triggered_at = None;
            }
        }

        let count = timestamps.len() as u32;
        let is_first = was_empty || count == 1;

        // 检查冷却期：回滚后的冷却期内不触发新进化
        let in_cooldown = if let Some(&cooldown_until) = self.cooldowns.get(skill_name) {
            if now < cooldown_until {
                true
            } else {
                // 冷却期已过，清除
                self.cooldowns.remove(skill_name);
                false
            }
        } else {
            false
        };

        // 检查是否应该触发进化：达到阈值 且 未在窗口期内触发过 且 不在冷却期
        let should_trigger = count >= self.threshold && triggered_at.is_none() && !in_cooldown;

        if should_trigger {
            // 标记已触发，但不清空计数器（保留历史用于统计）
            *triggered_at = Some(now);
            TrackResult {
                count,
                is_first,
                trigger: Some(TriggerReason::ConsecutiveFailures {
                    count,
                    window_minutes: self.window_minutes,
                }),
            }
        } else {
            TrackResult {
                count,
                is_first,
                trigger: None,
            }
        }
    }

    /// 清除某个技能的错误记录（进化成功后调用）
    pub(super) fn clear(&mut self, skill_name: &str) {
        self.errors.remove(skill_name);
    }

    /// 重置触发标记（允许再次触发进化）
    #[allow(dead_code)]
    pub(super) fn reset_trigger(&mut self, skill_name: &str) {
        if let Some(entry) = self.errors.get_mut(skill_name) {
            entry.1 = None;
        }
    }

    /// 设置冷却期（回滚后调用，避免立即重新触发进化）
    pub(super) fn set_cooldown(&mut self, skill_name: &str) {
        let cooldown_until = chrono::Utc::now().timestamp() + (self.cooldown_minutes as i64 * 60);
        self.cooldowns
            .insert(skill_name.to_string(), cooldown_until);
    }

    /// 检查某个技能是否在冷却期内
    #[allow(dead_code)]
    pub(super) fn is_in_cooldown(&self, skill_name: &str) -> bool {
        if let Some(&cooldown_until) = self.cooldowns.get(skill_name) {
            chrono::Utc::now().timestamp() < cooldown_until
        } else {
            false
        }
    }
}
