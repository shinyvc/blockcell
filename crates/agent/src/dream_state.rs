//! Layer 6 Dream State 辅助模块
//!
//! 由于 agent 和 scheduler 存在循环依赖，agent 无法直接引用 DreamConsolidator。
//! 此模块提供轻量级的 .dream_state.json 读写操作，用于在会话初始化时递增会话计数。

use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{info, warn};

/// .dream_state.json 的数据结构（与 scheduler 中的 DreamState 保持一致）
///
/// 注意：字段名和类型必须与 `scheduler::consolidator::DreamState` 完全匹配，
/// 否则 scheduler 反序列化失败会回退 default，导致 `current_session_count` 丢失。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DreamStateData {
    /// 上次整合时间戳（Unix 秒，u64 — 与 scheduler 一致）
    #[serde(default)]
    pub last_consolidation_time: Option<u64>,
    /// 上次整合时的会话数
    #[serde(default)]
    pub last_session_count: usize,
    /// 当前会话数
    #[serde(default)]
    pub current_session_count: usize,
    /// 整合次数
    #[serde(default)]
    pub consolidation_count: usize,
    /// 是否正在整合中
    #[serde(default)]
    pub is_consolidating: bool,
}

/// 获取 .dream_state.json 文件路径
fn dream_state_path(base_dir: &Path) -> std::path::PathBuf {
    base_dir.join(".dream_state.json")
}

/// 读取 .dream_state.json，如果不存在则返回默认值
async fn read_dream_state(base_dir: &Path) -> DreamStateData {
    let path = dream_state_path(base_dir);
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => DreamStateData::default(),
    }
}

/// 原子写入 .dream_state.json，使用 backup-based 策略保证原子性。
///
/// Windows 上 `rename` 在目标文件已存在时会失败，因此使用
/// backup-based 策略：old -> backup, tmp -> target, 失败时恢复 backup。
async fn write_dream_state(base_dir: &Path, state: &DreamStateData) -> std::io::Result<()> {
    let path = dream_state_path(base_dir);
    let content = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    crate::fs_util::atomic_write(&path, content.as_bytes())
}

/// 在会话初始化时递增会话计数并持久化
///
/// 此函数在 agent crate 的 init_memory_system 中调用，
/// 确保 Dream 整合的三重门控机制能正确判断会话数量。
pub async fn increment_dream_session_count(base_dir: &Path) {
    let mut state = read_dream_state(base_dir).await;
    state.current_session_count = state.current_session_count.saturating_add(1);

    if let Err(e) = write_dream_state(base_dir, &state).await {
        warn!(error = %e, "[layer6] 保存 .dream_state.json 失败");
    } else {
        info!(
            current_session_count = state.current_session_count,
            last_session_count = state.last_session_count,
            "[layer6] 会话计数已递增并持久化"
        );
    }
}
