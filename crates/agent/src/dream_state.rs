//! Layer 6 Dream State 辅助模块
//!
//! 由于 agent 和 scheduler 存在循环依赖，agent 无法直接引用 DreamConsolidator。
//! 此模块提供轻量级的 .dream_state.json 读写操作，用于在会话初始化时递增会话计数。

use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// .dream_state.json 的数据结构（与 scheduler 中的 DreamState 保持一致）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamStateData {
    /// 当前会话计数
    pub current_session_count: u64,
    /// 上次整合时的会话计数
    pub last_session_count: u64,
    /// 上次整合时间（Unix 时间戳秒）
    pub last_consolidation_at: Option<i64>,
    /// 是否正在整合中
    pub is_consolidating: bool,
}

impl Default for DreamStateData {
    fn default() -> Self {
        Self {
            current_session_count: 0,
            last_session_count: 0,
            last_consolidation_at: None,
            is_consolidating: false,
        }
    }
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

/// 写入 .dream_state.json
async fn write_dream_state(base_dir: &Path, state: &DreamStateData) -> std::io::Result<()> {
    let path = dream_state_path(base_dir);
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    tokio::fs::write(&path, content).await
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
