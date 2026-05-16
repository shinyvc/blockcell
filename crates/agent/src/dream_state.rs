//! Layer 6 Dream State 辅助模块
//!
//! 由于 agent 和 scheduler 存在循环依赖，agent 无法直接引用 DreamConsolidator。
//! 此模块提供轻量级的 .dream_state.json 读写操作，用于在会话初始化时递增会话计数。

use crate::auto_memory::CrossProcessLock;
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
    /// 整合开始时间戳（Unix 秒），用于 stale 检测
    #[serde(default)]
    pub consolidating_started_at: Option<u64>,
}

/// 获取 .dream_state.json 文件路径
fn dream_state_path(base_dir: &Path) -> std::path::PathBuf {
    base_dir.join(".dream_state.json")
}

/// 获取 .dream_state.json.lock 跨进程锁路径
///
/// 与 scheduler 侧 `consolidator::DreamState::save()` 使用相同的锁路径，
/// 确保 agent 和 scheduler 的 read-modify-write 序列互斥。
fn dream_state_lock_path(base_dir: &Path) -> std::path::PathBuf {
    dream_state_path(base_dir).with_extension("json.lock")
}

/// 读取 .dream_state.json，如果不存在则返回默认值。
/// 启动时检测 .bak.* 文件并恢复（崩溃恢复逻辑）。
async fn read_dream_state(base_dir: &Path) -> DreamStateData {
    let path = dream_state_path(base_dir);

    // 崩溃恢复：如果主文件不存在但存在备份文件，尝试恢复
    // 使用 find_latest_backup 查找 atomic_write 产生的最新备份
    // （备份文件名格式为 .dream_state.json.bak.<pid>.<counter>）
    if !path.exists() {
        if let Some(bak_path) = crate::fs_util::find_latest_backup(&path) {
            warn!(
                path = %path.display(),
                bak = %bak_path.display(),
                "[layer6] 主文件不存在但发现备份文件，尝试恢复"
            );
            if let Ok(bak_content) = tokio::fs::read_to_string(&bak_path).await {
                if let Ok(state) = serde_json::from_str::<DreamStateData>(&bak_content) {
                    // 恢复成功：将备份内容写入主文件
                    if let Ok(write_content) = serde_json::to_string_pretty(&state) {
                        if tokio::fs::write(&path, write_content).await.is_ok() {
                            info!("[layer6] 从备份文件恢复成功");
                            return state;
                        }
                    }
                }
            }
            warn!("[layer6] 从备份文件恢复失败，使用默认值");
        }
        return DreamStateData::default();
    }

    match tokio::fs::read_to_string(&path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            warn!(error = %e, "[layer6] 解析 .dream_state.json 失败，使用默认值");
            DreamStateData::default()
        }),
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
///
/// 使用跨进程锁保护 read-modify-write 序列，防止并发会话初始化导致计数丢失。
pub async fn increment_dream_session_count(base_dir: &Path) {
    let lock_path = dream_state_lock_path(base_dir);
    let _lock_guard = match CrossProcessLock::acquire(&lock_path) {
        Ok(guard) => guard,
        Err(e) => {
            warn!(error = %e, "[layer6] 获取跨进程锁失败，继续非原子递增");
            // Fallback: proceed without lock (best-effort)
            let mut state = read_dream_state(base_dir).await;
            state.current_session_count = state.current_session_count.saturating_add(1);
            if let Err(e) = write_dream_state(base_dir, &state).await {
                warn!(error = %e, "[layer6] 保存 .dream_state.json 失败");
            }
            return;
        }
    };

    let mut state = read_dream_state(base_dir).await;
    state.current_session_count = state.current_session_count.saturating_add(1);

    if let Err(e) = write_dream_state(base_dir, &state).await {
        warn!(error = %e, "[layer6] 保存 .dream_state.json 失败");
    } else {
        info!(
            current_session_count = state.current_session_count,
            last_session_count = state.last_session_count,
            "[layer6] 会话计数已递增并持久化（跨进程锁保护）"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试：用缺字段的旧 JSON 反序列化，确认不会失败且缺失字段走默认值
    #[test]
    fn test_dream_state_data_partial_json_deserialization() {
        // 旧版本 JSON 只有部分字段
        let partial_json = r#"{"current_session_count": 5}"#;
        let state: DreamStateData = serde_json::from_str(partial_json).unwrap();
        assert_eq!(state.current_session_count, 5);
        assert_eq!(state.last_consolidation_time, None);
        assert_eq!(state.last_session_count, 0);
        assert_eq!(state.consolidation_count, 0);
        assert!(!state.is_consolidating);

        // 完全空的 JSON 对象
        let empty_json = r#"{}"#;
        let state: DreamStateData = serde_json::from_str(empty_json).unwrap();
        assert_eq!(state.current_session_count, 0);
        assert_eq!(state.last_consolidation_time, None);

        // 只有 consolidation_count 的 JSON
        let single_field_json = r#"{"consolidation_count": 3}"#;
        let state: DreamStateData = serde_json::from_str(single_field_json).unwrap();
        assert_eq!(state.consolidation_count, 3);
        assert_eq!(state.current_session_count, 0);
    }

    /// 测试：完整 JSON 的序列化/反序列化往返
    #[test]
    fn test_dream_state_data_roundtrip() {
        let state = DreamStateData {
            last_consolidation_time: Some(1234567890),
            last_session_count: 10,
            current_session_count: 15,
            consolidation_count: 3,
            is_consolidating: false,
            consolidating_started_at: Some(1234567800),
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let restored: DreamStateData = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.last_consolidation_time, Some(1234567890));
        assert_eq!(restored.last_session_count, 10);
        assert_eq!(restored.current_session_count, 15);
        assert_eq!(restored.consolidation_count, 3);
        assert!(!restored.is_consolidating);
        assert_eq!(restored.consolidating_started_at, Some(1234567800));
    }

    /// 测试：崩溃恢复 — 主文件不存在但备份文件存在时恢复
    ///
    /// 创建 `.dream_state.json.bak.<pid>.<counter>` 格式的备份文件，
    /// 与 `find_latest_backup()` 的查找前缀匹配。
    #[tokio::test]
    async fn test_dream_state_crash_recovery_from_backup() {
        let tmp = std::env::temp_dir().join("test_dream_state_crash_recovery");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // 先写入一个正常的 dream state
        let state = DreamStateData {
            current_session_count: 42,
            consolidation_count: 5,
            last_consolidation_time: Some(999),
            ..Default::default()
        };
        write_dream_state(&tmp, &state).await.unwrap();

        // 模拟崩溃：创建正确格式的备份文件，删除主文件
        let main_path = tmp.join(".dream_state.json");
        // 使用 atomic_write 产生备份（Windows 上会自动产生 .bak.* 文件）
        // 在所有平台上，手动创建一个 .bak.<pid>.<counter> 格式的备份文件
        let main_content = std::fs::read_to_string(&main_path).unwrap();
        let bak_path = tmp.join(format!(".dream_state.json.bak.{}.0", std::process::id()));
        std::fs::write(&bak_path, &main_content).unwrap();
        // 删除主文件
        std::fs::remove_file(&main_path).unwrap();

        // 读取时应该从备份恢复
        let restored = read_dream_state(&tmp).await;
        assert_eq!(restored.current_session_count, 42);
        assert_eq!(restored.consolidation_count, 5);
        assert_eq!(restored.last_consolidation_time, Some(999));

        // 主文件应该被恢复
        assert!(main_path.exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
