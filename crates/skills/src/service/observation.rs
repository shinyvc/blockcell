//! 部署后观察期的执行统计追踪器。
//!
//! 记录每个 evolution 在观察窗口内的调用总数与错误数，用于计算错误率。

use std::collections::HashMap;

/// 观察期统计追踪器：记录部署后观察窗口内的执行统计
#[derive(Debug, Clone, Default)]
pub(super) struct ObservationStats {
    /// evolution_id -> (total_calls, error_calls)
    pub(super) active: HashMap<String, (u64, u64)>,
}

impl ObservationStats {
    /// 记录一次技能调用结果
    pub(super) fn record_call(&mut self, evolution_id: &str, is_error: bool) {
        let entry = self
            .active
            .entry(evolution_id.to_string())
            .or_insert((0, 0));
        entry.0 += 1;
        if is_error {
            entry.1 += 1;
        }
    }

    /// 获取当前错误率
    pub(super) fn error_rate(&self, evolution_id: &str) -> f64 {
        if let Some(&(total, errors)) = self.active.get(evolution_id) {
            if total == 0 {
                0.0
            } else {
                errors as f64 / total as f64
            }
        } else {
            0.0
        }
    }

    /// 移除已完成的 evolution
    pub(super) fn remove(&mut self, evolution_id: &str) {
        self.active.remove(evolution_id);
    }
}
