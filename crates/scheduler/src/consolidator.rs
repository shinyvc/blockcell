//! 梦境机制 - Layer 6 知识整合
//!
//! 后台跨会话知识整合，使用三重门控机制。
//!
//! ## 三重门控
//! 1. 时间门控：距上次整合 > 24 小时
//! 2. 会话门控：新会话数 > 5
//! 3. 锁门控：无其他进程正在整合
//!
//! ## 四阶段执行
//! 1. Orient - 定位现有内容
//! 2. Gather - 收集新信号
//! 3. Consolidate - 整合知识（使用 Forked Agent）
//! 4. Prune - 修剪索引

use blockcell_agent::forked::{
    build_forked_tool_schemas, create_dream_can_use_tool, run_forked_agent, CacheSafeParams,
    ForkedAgentParams, ForkedAgentResult,
};
use blockcell_agent::memory_event;
use blockcell_agent::session_metrics::get_dream_circuit_breaker;
use blockcell_agent::CrossProcessLock;
use blockcell_core::types::ChatMessage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;

/// 门控配置
pub const TIME_GATE_THRESHOLD_HOURS: u64 = 24;
pub const SESSION_GATE_THRESHOLD: usize = 5;
pub const LOCK_FILE_NAME: &str = ".dream_lock";
pub const DREAM_STATE_FILE: &str = ".dream_state.json";

/// Session Memory 过期阈值（天）
pub const SESSION_MEMORY_EXPIRY_DAYS: u64 = 7;
/// 每次处理的最大 session memory 文件数
pub const MAX_SESSIONS_TO_PROCESS: usize = 10;
/// is_consolidating 标记的 stale 阈值（秒）
///
/// 超过此时间仍为 is_consolidating=true 时，视为上次整合异常退出留下的 stale 标记，
/// gate 自动清除并允许新的整合。默认 1 小时，远大于正常整合超时（300s）。
pub const CONSOLIDATING_STALE_THRESHOLD_SECS: u64 = 3600;

/// Dream 执行统计数据
#[derive(Debug, Clone, Default)]
pub struct DreamStats {
    /// 创建的记忆数
    pub memories_created: usize,
    /// 更新的记忆数
    pub memories_updated: usize,
    /// 删除的记忆数
    pub memories_deleted: usize,
    /// 修剪的会话数
    pub sessions_pruned: usize,
    /// 处理的会话数
    pub sessions_processed: usize,
}

impl DreamStats {
    fn has_memory_changes(&self) -> bool {
        self.memories_created > 0 || self.memories_updated > 0 || self.memories_deleted > 0
    }
}

/// Memory 目录状态快照
#[derive(Debug, Clone, Default)]
struct MemoryDirState {
    /// 文件数量 (保留用于未来日志/指标)
    #[allow(dead_code)]
    file_count: usize,
    /// 总字节数 (保留用于未来日志/指标)
    #[allow(dead_code)]
    total_bytes: u64,
    /// 文件名 -> 修改时间映射
    file_mtimes: std::collections::HashMap<String, u64>,
}

/// 梦境状态
///
/// 字段必须添加 `#[serde(default)]` 以保证向后兼容：
/// 当新增字段时，旧版 .dream_state.json 文件仍能正确反序列化。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DreamState {
    /// 上次整合时间戳
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
    /// 是否正在整合
    #[serde(default)]
    pub is_consolidating: bool,
    /// 整合开始时间戳（Unix 秒），用于检测 stale 的 is_consolidating 标记
    ///
    /// 当 is_consolidating=true 但 consolidating_started_at 超过
    /// CONSOLIDATING_STALE_THRESHOLD_SECS 时，视为 stale 标记，
    /// gate 自动清除并允许新的整合。
    #[serde(default)]
    pub consolidating_started_at: Option<u64>,
}

impl DreamState {
    /// 加载状态
    pub async fn load(config_dir: &Path) -> std::io::Result<Self> {
        let path = config_dir.join(DREAM_STATE_FILE);
        match fs::read_to_string(&path).await {
            Ok(content) => {
                match serde_json::from_str(&content) {
                    Ok(state) => Ok(state),
                    Err(e) => {
                        // JSON 解析失败，可能文件损坏，记录警告并使用默认值
                        tracing::warn!(
                            error = %e,
                            path = %path.display(),
                            "[dream] Failed to parse dream state file, using defaults (file may be corrupted)"
                        );
                        Ok(Self::default())
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 主文件不存在，尝试从 atomic_write 产生的备份恢复
                // 备份文件名格式为 .dream_state.json.bak.<pid>.<counter>
                let bak_path = blockcell_agent::fs_util::find_latest_backup(&path);
                if let Some(bak) = bak_path {
                    tracing::warn!(
                        path = %path.display(),
                        bak = %bak.display(),
                        "[dream] 主文件不存在但发现备份文件，尝试恢复"
                    );
                    match fs::read_to_string(&bak).await {
                        Ok(bak_content) => {
                            match serde_json::from_str(&bak_content) {
                                Ok(state) => {
                                    // 恢复成功：将备份内容写入主文件
                                    let write_content = serde_json::to_string_pretty(&state)?;
                                    tokio::task::spawn_blocking(move || {
                                        blockcell_agent::fs_util::atomic_write(
                                            &path,
                                            write_content.as_bytes(),
                                        )
                                    })
                                    .await
                                    .map_err(|e| std::io::Error::other(e.to_string()))?
                                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                                    tracing::info!("[dream] 从备份文件恢复成功");
                                    Ok(state)
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "[dream] 解析备份文件失败，使用默认值");
                                    Ok(Self::default())
                                }
                            }
                        }
                        Err(_) => {
                            tracing::warn!("[dream] 读取备份文件失败，使用默认值");
                            Ok(Self::default())
                        }
                    }
                } else {
                    Ok(Self::default())
                }
            }
            Err(e) => Err(e),
        }
    }

    /// 保存状态（原子写入 + 跨进程锁，防止并发写入和崩溃导致文件损坏）
    ///
    /// 使用与 agent 侧 `increment_dream_session_count()` 相同的锁文件
    /// `.dream_state.json.lock`，确保 scheduler 和 agent 的 read-modify-write
    /// 序列互斥，避免 TOCTOU 竞争导致计数丢失。
    ///
    /// 获取锁失败时返回错误，不再继续非原子写入，
    /// 避免在锁被其他进程持有时引入覆盖 session count 的风险。
    pub async fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        let lock_path = config_dir
            .join(DREAM_STATE_FILE)
            .with_extension("json.lock");
        let _lock_guard = CrossProcessLock::acquire(&lock_path).map_err(|e| {
            tracing::warn!(
                error = %e,
                "[dream] save: 获取跨进程锁失败，拒绝写入以防止覆盖风险"
            );
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!("获取跨进程锁失败，拒绝非原子写入: {}", e),
            )
        })?;

        let path = config_dir.join(DREAM_STATE_FILE);
        let content = serde_json::to_string_pretty(self)?;
        // 使用 blockcell_agent::fs_util::atomic_write 保证原子性
        // （backup-based 策略，Windows 安全）
        let write_result = tokio::task::spawn_blocking(move || {
            blockcell_agent::fs_util::atomic_write(&path, content.as_bytes())
        })
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        write_result.map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }

    /// 保存状态，不获取跨进程锁。
    ///
    /// 供已持有 `.dream_state.json.lock` 的调用者使用（如 `dream()` 最终合并），
    /// 避免持锁后再调用会重新抢锁的 `save()`，导致死锁或锁间隙。
    async fn save_unlocked(&self, config_dir: &Path) -> std::io::Result<()> {
        let path = config_dir.join(DREAM_STATE_FILE);
        let content = serde_json::to_string_pretty(self)?;
        let write_result = tokio::task::spawn_blocking(move || {
            blockcell_agent::fs_util::atomic_write(&path, content.as_bytes())
        })
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        write_result.map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }

    /// 增加会话计数
    pub fn increment_session_count(&mut self) {
        self.current_session_count += 1;
    }
}

fn apply_successful_dream_state(
    state: &mut DreamState,
    stats: &DreamStats,
    processed_session_count: usize,
    now_secs: u64,
) {
    state.last_consolidation_time = Some(now_secs);
    // 成功执行过的会话批次都要推进游标；即使本轮没有产出记忆变更，
    // 也不能让下一次时间门打开后反复处理同一批会话。
    state.last_session_count = processed_session_count;

    if stats.has_memory_changes() {
        state.consolidation_count += 1;
    } else {
        tracing::info!("[dream] consolidation produced no changes");
    }
}

fn validate_dream_agent_result(agent_result: &ForkedAgentResult) -> Result<(), String> {
    if agent_result.had_tool_error {
        return Err("Forked Agent 工具调用失败 (had_tool_error=true)".to_string());
    }

    if agent_result.truncated {
        return Err("Forked Agent reached max_turns before finishing (truncated=true)".to_string());
    }

    Ok(())
}

/// 收集到的信号
#[derive(Debug, Clone)]
pub struct GatheredSignal {
    /// 信号标题（章节名）
    pub title: String,
    /// 信号内容
    pub content: String,
    /// 重要性分数 (0-10)
    pub importance: u8,
    /// 来源时间
    pub source_time: SystemTime,
}

/// 门控检查结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateCheckResult {
    /// 通过所有门控
    Passed,
    /// 时间门控未通过
    TimeGateFailed,
    /// 会话门控未通过
    SessionGateFailed,
    /// 锁门控未通过（有其他进程正在整合）
    LockGateFailed,
}

/// 整合器配置（从 DreamServiceConfig 传入，覆盖硬编码常量）
#[derive(Debug, Clone)]
pub struct ConsolidatorConfig {
    /// 时间门限阈值（小时）
    pub time_gate_threshold_hours: f64,
    /// 会话门限阈值（会话数）
    pub session_gate_threshold: usize,
}

impl Default for ConsolidatorConfig {
    fn default() -> Self {
        Self {
            time_gate_threshold_hours: TIME_GATE_THRESHOLD_HOURS as f64,
            session_gate_threshold: SESSION_GATE_THRESHOLD,
        }
    }
}

/// 三重门控检查（使用配置值）
///
/// 当 is_consolidating=true 但 consolidating_started_at 超过 stale 阈值时，
/// 自动清除 stale 标记并持久化，允许新的整合继续。
/// 这防止了因临时磁盘/锁异常导致 is_consolidating 永久卡住的问题。
pub async fn check_gates(
    state: &mut DreamState,
    config_dir: &Path,
    config: &ConsolidatorConfig,
) -> GateCheckResult {
    // 1. 检查锁门控
    if state.is_consolidating {
        // 检查是否为 stale 标记：consolidating_started_at 超过阈值
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let is_stale = match state.consolidating_started_at {
            Some(started_at) => {
                // 有开始时间，检查是否超过 stale 阈值
                now.saturating_sub(started_at) > CONSOLIDATING_STALE_THRESHOLD_SECS
            }
            None => {
                // is_consolidating=true 但没有 consolidating_started_at，
                // 说明是旧格式数据或异常状态。
                // 复用 .dream_lock 的有效性检查逻辑：
                // 如果 lock 文件不存在或已失效（进程退出/超时），标记一定是 stale。
                let lock_path = config_dir.join(LOCK_FILE_NAME);
                if !lock_path.exists() {
                    // 锁文件不存在，没有进程正在整合，标记是 stale
                    true
                } else {
                    // 锁文件存在，但需要检查其有效性（进程是否存活、是否超时）
                    match check_lock_validity(&lock_path).await {
                        Ok(true) => {
                            // 锁仍有效（进程存活且未过期），不是 stale
                            tracing::warn!(
                                "[dream] is_consolidating=true 但无 consolidating_started_at，且 .dream_lock 仍有效，gate 阻止新整合"
                            );
                            false
                        }
                        Ok(false) => {
                            // 锁已失效（进程退出或超时），标记是 stale
                            tracing::warn!(
                                "[dream] is_consolidating=true 但无 consolidating_started_at，且 .dream_lock 已失效，自动清除 stale 标记并清理无效锁"
                            );
                            // 清理无效的锁文件
                            let _ = fs::remove_file(&lock_path).await;
                            true
                        }
                        Err(e) => {
                            // 无法读取锁文件，视为 stale 并清理
                            tracing::warn!(
                                error = %e,
                                "[dream] is_consolidating=true 但无 consolidating_started_at，无法读取 .dream_lock，视为 stale 并清理"
                            );
                            let _ = fs::remove_file(&lock_path).await;
                            true
                        }
                    }
                }
            }
        };

        if is_stale {
            tracing::warn!(
                started_at = ?state.consolidating_started_at,
                "[dream] 检测到 stale 的 is_consolidating 标记，自动清除恢复"
            );
            state.is_consolidating = false;
            state.consolidating_started_at = None;
            // 持久化清除后的状态，确保后续 gate 不再被卡住
            if let Err(e) = state.save(config_dir).await {
                tracing::warn!(
                    error = %e,
                    "[dream] 清除 stale 标记后保存状态失败，下次 gate 会再次尝试清除"
                );
                // 保存失败不阻止 gate 通过：内存中已清除，下次加载会重新检测
            }
        } else {
            return GateCheckResult::LockGateFailed;
        }
    }

    // 2. 检查时间门控
    if let Some(last_time) = state.last_consolidation_time {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let hours_since_last = (now.saturating_sub(last_time)) as f64 / 3600.0;

        if hours_since_last < config.time_gate_threshold_hours {
            return GateCheckResult::TimeGateFailed;
        }
    } else {
        // 从未整合过，时间门控通过
    }

    // 3. 检查会话门控
    let new_sessions = state
        .current_session_count
        .saturating_sub(state.last_session_count);
    if new_sessions < config.session_gate_threshold {
        return GateCheckResult::SessionGateFailed;
    }

    memory_event!(layer6, gate_passed, "all_gates");
    GateCheckResult::Passed
}

/// 梦境执行器
pub struct DreamConsolidator {
    /// 配置目录
    config_dir: PathBuf,
    /// 当前状态
    state: DreamState,
    /// 门控配置
    gate_config: ConsolidatorConfig,
    /// Provider 池（用于 Forked Agent LLM 调用）
    provider_pool: Option<Arc<blockcell_providers::ProviderPool>>,
}

impl DreamConsolidator {
    /// 创建执行器
    pub async fn new(config_dir: &Path) -> std::io::Result<Self> {
        let state = DreamState::load(config_dir).await?;
        Ok(Self {
            config_dir: config_dir.to_path_buf(),
            state,
            gate_config: ConsolidatorConfig::default(),
            provider_pool: None,
        })
    }

    /// 使用自定义门控配置
    pub fn with_gate_config(mut self, config: ConsolidatorConfig) -> Self {
        self.gate_config = config;
        self
    }

    /// 设置 Provider 池
    ///
    /// 必须在调用 `dream()` 之前设置，否则 Forked Agent 无法执行 LLM 调用
    pub fn with_provider_pool(
        mut self,
        provider_pool: Arc<blockcell_providers::ProviderPool>,
    ) -> Self {
        self.provider_pool = Some(provider_pool);
        self
    }

    /// 检查是否应该执行梦境
    pub async fn should_dream(&mut self) -> GateCheckResult {
        check_gates(&mut self.state, &self.config_dir, &self.gate_config).await
    }

    /// 执行梦境整合
    ///
    /// timeout_secs: 单次整合的超时时间（秒），超时后仍会执行清理逻辑
    pub async fn dream(&mut self, timeout_secs: u64) -> Result<(), DreamError> {
        // 获取锁
        self.acquire_lock().await?;

        // 记录 Layer 6 dream_started 事件
        let sessions_count = self.state.current_session_count;
        let hours_since_last = self
            .state
            .last_consolidation_time
            .map(|t| {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                (now.saturating_sub(t)) / 3600
            })
            .unwrap_or(24);
        memory_event!(layer6, dream_started, sessions_count, hours_since_last);

        // 标记开始（同时记录开始时间戳，用于 stale 检测）
        self.state.is_consolidating = true;
        self.state.consolidating_started_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
        if let Err(e) = self.state.save(&self.config_dir).await {
            // 保存失败，重置状态并释放锁
            self.state.is_consolidating = false;
            self.state.consolidating_started_at = None;
            let _ = self.release_lock().await;
            return Err(DreamError::Io(e));
        }

        // 在整合开始前保存当前会话数快照，用于成功后推进 last_session_count。
        // 避免整合期间新增的会话被误标为已整合（它们未必被本次 gather/prune 处理）。
        let processed_session_count = self.state.current_session_count;

        let start_time = Instant::now();

        // 在 consolidate 前扫描 memory 目录
        let memory_dir = self.config_dir.join("memory");
        let pre_memory_state = self.scan_memory_dir(&memory_dir).await;

        // 执行四阶段（带超时保护），收集统计
        // 超时后不会 drop 整个 dream()，而是返回 Err(DreamError::Timeout)，
        // 确保后续清理逻辑（is_consolidating=false、保存状态、释放锁）始终执行
        let mut stats = DreamStats::default();
        let result = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async {
            self.orient().await?;
            memory_event!(layer6, phase_completed, "orient");
            let signals = self.gather().await?;
            memory_event!(layer6, phase_completed, "gather");
            self.consolidate(&signals).await?;
            memory_event!(layer6, phase_completed, "consolidate");
            // 在 consolidate 后计算 memory 变化
            let post_memory_state = self.scan_memory_dir(&memory_dir).await;
            stats = self.compute_memory_diff(&pre_memory_state, &post_memory_state);
            // prune 返回修剪统计
            let prune_stats = self.prune().await?;
            memory_event!(layer6, phase_completed, "prune");
            stats.sessions_pruned = prune_stats.sessions_pruned;
            stats.sessions_processed = prune_stats.sessions_processed;
            Ok::<(), DreamError>(())
        })
        .await
        .map_err(|_| {
            tracing::error!(
                timeout_secs,
                "[dream] Consolidation timed out, executing cleanup"
            );
            DreamError::Timeout(timeout_secs)
        })
        .and_then(|r| r);

        // 清理：无论成功、失败或超时，都要释放锁和重置标记
        self.state.is_consolidating = false;
        self.state.consolidating_started_at = None;

        // 只有成功时才推进时间门和会话门，失败/超时保留原值以便重试
        if result.is_ok() {
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            apply_successful_dream_state(
                &mut self.state,
                &stats,
                processed_session_count,
                now_secs,
            );
        }

        // 最终保存：在同一个跨进程锁保护下完成 read-merge-write，
        // 防止 agent 在 load 和 save 之间递增 session_count 并被覆盖。
        //
        // 关键：is_consolidating=false 必须落盘，否则后续 gate 永远 LockGateFailed。
        // 获取锁失败或 save_unlocked 失败时，必须重试或返回错误，
        // 不能让调用方看到成功但磁盘上仍为 is_consolidating=true。
        {
            let state_lock_path = self
                .config_dir
                .join(DREAM_STATE_FILE)
                .with_extension("json.lock");

            // 重试获取状态锁，最多 3 次（间隔递增），确保 is_consolidating=false 能落盘
            let state_lock_guard = {
                let mut guard_result = CrossProcessLock::acquire(&state_lock_path);
                let mut retry_count = 0;
                const MAX_STATE_LOCK_RETRIES: u32 = 3;
                while let Err(e) = guard_result {
                    retry_count += 1;
                    if retry_count > MAX_STATE_LOCK_RETRIES {
                        tracing::error!(
                            error = %e,
                            retries = retry_count,
                            "[dream] 获取状态锁失败（已重试 {retry_count} 次），is_consolidating=false 无法落盘，返回错误"
                        );
                        // 释放 dream lock
                        if let Err(e) = self.release_lock().await {
                            tracing::warn!(error = %e, "[dream] Failed to release lock");
                        }
                        // 返回错误而非成功：调用方必须知道状态未持久化
                        return Err(DreamError::Io(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            format!(
                                "获取状态锁失败（重试 {} 次），is_consolidating=false 无法落盘: {}",
                                retry_count, e
                            ),
                        )));
                    }
                    tracing::warn!(
                        error = %e,
                        retry = retry_count,
                        "[dream] 获取状态锁失败，重试中"
                    );
                    // 递增等待：100ms, 200ms, 300ms
                    tokio::time::sleep(std::time::Duration::from_millis(100 * retry_count as u64))
                        .await;
                    guard_result = CrossProcessLock::acquire(&state_lock_path);
                }
                let guard = match guard_result {
                    Ok(g) => g,
                    Err(e) => {
                        return Err(DreamError::Io(std::io::Error::other(format!(
                            "获取 dream 状态锁失败: {}",
                            e
                        ))))
                    }
                };
                guard
            };

            // 在锁内重新读取磁盘上的 current_session_count，
            // 合并整合期间 agent 递增的增量。
            // current_session_count 可以 merge 磁盘较大值（反映真实总数），
            // 但 last_session_count 只推进到整合开始前的快照值，
            // 防止整合期间新增的会话被误标为已整合。
            if result.is_ok() {
                if let Ok(disk) = DreamState::load(&self.config_dir).await {
                    self.state.current_session_count =
                        std::cmp::max(self.state.current_session_count, disk.current_session_count);
                    // last_session_count 已在上方设为 processed_session_count，
                    // 不再使用 merged current_session_count 更新它
                }
            }

            // 保存最终状态（使用 save_unlocked 避免重复抢锁）
            // 失败时重试最多 2 次，确保 is_consolidating=false 落盘
            let mut save_retry_count = 0;
            const MAX_SAVE_RETRIES: u32 = 2;
            loop {
                match self.state.save_unlocked(&self.config_dir).await {
                    Ok(()) => break,
                    Err(e) => {
                        save_retry_count += 1;
                        if save_retry_count > MAX_SAVE_RETRIES {
                            tracing::error!(
                                error = %e,
                                retries = save_retry_count,
                                "[dream] 最终状态保存失败（已重试 {save_retry_count} 次），is_consolidating=false 未落盘，返回错误"
                            );
                            // 状态锁在 drop 时自动释放
                            drop(state_lock_guard);
                            // 释放 dream lock
                            if let Err(e) = self.release_lock().await {
                                tracing::warn!(error = %e, "[dream] Failed to release lock");
                            }
                            // 返回错误而非成功：调用方必须知道 is_consolidating=false 未落盘
                            return Err(DreamError::Io(e));
                        }
                        tracing::warn!(
                            error = %e,
                            retry = save_retry_count,
                            "[dream] 最终状态保存失败，重试中"
                        );
                    }
                }
            }

            // 状态锁在 _state_lock_guard drop 时自动释放
            drop(state_lock_guard);
        }

        // 释放锁（失败时记录警告但继续）
        if let Err(e) = self.release_lock().await {
            tracing::warn!(
                error = %e,
                "[dream] Failed to release lock"
            );
        }

        let elapsed = start_time.elapsed();
        match &result {
            Ok(()) => {
                // 记录 Layer 6 dream_finished 事件（成功，传递实际统计数据）
                memory_event!(
                    layer6,
                    dream_finished,
                    stats.memories_created,
                    stats.memories_updated,
                    stats.memories_deleted,
                    stats.sessions_pruned,
                    stats.sessions_processed
                );
                tracing::info!(
                    elapsed_ms = elapsed.as_millis() as u64,
                    consolidation_count = self.state.consolidation_count,
                    memories_created = stats.memories_created,
                    memories_updated = stats.memories_updated,
                    sessions_pruned = stats.sessions_pruned,
                    "[dream] consolidation completed"
                );
            }
            Err(e) => {
                memory_event!(layer6, dream_failed, e.to_string());
                tracing::error!(
                    elapsed_ms = elapsed.as_millis() as u64,
                    error = %e,
                    "[dream] consolidation failed"
                );
            }
        }

        result
    }

    /// 获取锁
    ///
    /// 使用原子 rename 操作避免 TOCTOU 竞争条件。
    /// 锁文件格式: `PID:TIMESTAMP`
    ///
    /// ## 算法
    /// 1. 先创建临时锁文件（带唯一标识）
    /// 2. 检查现有锁是否过期
    /// 3. 如果过期，尝试原子 rename（只有一个进程会成功）
    /// 4. 如果 rename 失败，说明另一个进程已获取锁
    async fn acquire_lock(&self) -> Result<(), DreamError> {
        use std::process;

        let lock_path = self.config_dir.join(LOCK_FILE_NAME);
        let temp_lock_path =
            self.config_dir
                .join(format!("{}.tmp.{}", LOCK_FILE_NAME, process::id()));
        let current_pid = process::id();
        let max_retries = 3;

        for attempt in 0..max_retries {
            // 1. 先创建临时锁文件（每个进程有自己的临时文件，无竞争）
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let lock_content = format!("{}:{}", current_pid, timestamp);

            // 确保配置目录存在
            if let Some(parent) = lock_path.parent() {
                fs::create_dir_all(parent).await?;
            }

            // 写入临时文件
            fs::write(&temp_lock_path, &lock_content).await?;

            // 2. 检查现有锁是否存在且有效
            match fs::try_exists(&lock_path).await {
                Ok(true) => {
                    // 锁文件存在，检查是否过期
                    match check_lock_validity(&lock_path).await {
                        Ok(true) => {
                            // 锁仍然有效，清理临时文件并返回
                            tracing::debug!(attempt, "[dream] Lock is held by another process");
                            let _ = fs::remove_file(&temp_lock_path).await;
                            return Err(DreamError::LockAcquired);
                        }
                        Ok(false) => {
                            // 锁已过期，尝试原子替换
                            // rename 在大多数平台上是原子的
                            match fs::rename(&temp_lock_path, &lock_path).await {
                                Ok(()) => {
                                    tracing::debug!(
                                        pid = current_pid,
                                        attempt,
                                        "[dream] Lock acquired (replaced stale lock)"
                                    );
                                    return Ok(());
                                }
                                Err(e) => {
                                    // rename 失败，可能另一个进程已获取锁
                                    tracing::warn!(
                                        error = %e,
                                        attempt,
                                        "[dream] Failed to replace stale lock, retrying"
                                    );
                                    let _ = fs::remove_file(&temp_lock_path).await;
                                    // 继续重试
                                }
                            }
                        }
                        Err(e) => {
                            // 无法读取锁文件，尝试替换
                            tracing::warn!(
                                error = %e,
                                "[dream] Cannot read lock file, attempting to replace"
                            );
                            match fs::rename(&temp_lock_path, &lock_path).await {
                                Ok(()) => {
                                    tracing::debug!(
                                        pid = current_pid,
                                        "[dream] Lock acquired (replaced corrupted lock)"
                                    );
                                    return Ok(());
                                }
                                Err(_e) => {
                                    let _ = fs::remove_file(&temp_lock_path).await;
                                }
                            }
                        }
                    }
                }
                Ok(false) => {
                    // 锁文件不存在，尝试创建
                    match fs::rename(&temp_lock_path, &lock_path).await {
                        Ok(()) => {
                            tracing::debug!(
                                pid = current_pid,
                                attempt,
                                "[dream] Lock acquired (new lock)"
                            );
                            return Ok(());
                        }
                        Err(e) => {
                            // rename 失败（可能另一个进程同时创建）
                            tracing::warn!(
                                error = %e,
                                attempt,
                                "[dream] Failed to create lock, retrying"
                            );
                            let _ = fs::remove_file(&temp_lock_path).await;
                            // 继续重试
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "[dream] Cannot check lock existence"
                    );
                    let _ = fs::remove_file(&temp_lock_path).await;
                    return Err(e.into());
                }
            }
        }

        // 重试次数耗尽
        tracing::error!(
            attempts = max_retries,
            "[dream] Failed to acquire lock after max retries"
        );
        // 清理临时文件
        let _ = fs::remove_file(&temp_lock_path).await;
        Err(DreamError::LockAcquired)
    }

    /// 释放锁
    async fn release_lock(&self) -> Result<(), DreamError> {
        let lock_path = self.config_dir.join(LOCK_FILE_NAME);
        if fs::try_exists(&lock_path).await? {
            fs::remove_file(&lock_path).await?;
        }
        Ok(())
    }

    /// 阶段 1: 定位现有内容
    async fn orient(&self) -> Result<(), DreamError> {
        tracing::debug!("[dream] Phase 1: Orienting");

        // 读取现有记忆文件，建立索引
        let memory_dir = self.config_dir.join("memory");
        if !fs::try_exists(&memory_dir).await? {
            fs::create_dir_all(&memory_dir).await?;
        }

        Ok(())
    }

    /// 阶段 2: 收集新信号
    ///
    /// 从 session memory 文件中收集信息，提取需要整合的信号。
    /// 优先级：最新的会话 > 旧的会话
    async fn gather(&self) -> Result<Vec<GatheredSignal>, DreamError> {
        tracing::debug!("[dream] Phase 2: Gathering signals");

        let mut signals = Vec::new();
        let sessions_dir = self.config_dir.join("sessions");

        if !fs::try_exists(&sessions_dir).await? {
            return Ok(signals);
        }

        // 收集所有 session memory 文件及其修改时间
        let mut session_files: Vec<(PathBuf, SystemTime)> = Vec::new();
        let mut entries = fs::read_dir(&sessions_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            // 跳过非目录条目（如 .DS_Store）
            if entry.file_type().await.map(|t| !t.is_dir()).unwrap_or(true) {
                continue;
            }
            // 避让正在提取的 Session Memory：
            // 如果目录下存在 .extraction_pending 标记，说明 Layer 3 正在更新 memory.md，
            // 此时读取可能得到旧内容或写入中的半截内容，应跳过。
            // 但如果标记已过期（超过 stale 阈值），说明提取任务已崩溃或被遗弃，
            // 清理过期标记和对应 journal 后继续读取当前 memory.md。
            let pending_marker = entry.path().join(".extraction_pending");
            if fs::try_exists(&pending_marker).await.unwrap_or(false) {
                // stale 阈值：10x Layer3 默认 extraction_stale_threshold (60s * 10 = 600s)
                // 此常量与 agent 侧 Layer3Config::extraction_stale_threshold_ms (默认 60000ms) 关联。
                // scheduler crate 无法直接引用 agent 配置，故使用关联常量。
                // 10x 裕量确保：即使 LLM 提取耗时较长，也不会被 Dream 误清。
                const EXTRACTION_STALE_THRESHOLD_SECS: u64 = 600;
                let is_mtime_stale = fs::metadata(&pending_marker)
                    .await
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|mtime| mtime.elapsed().ok())
                    .map(|elapsed| elapsed.as_secs() >= EXTRACTION_STALE_THRESHOLD_SECS)
                    .unwrap_or(true); // 无法读取 mtime 视为过期

                if is_mtime_stale {
                    // marker 已过期，但在清理前先检查 journal 的 owner_pid/started_at
                    // 避免 Dream 误删长耗时任务的 marker
                    let journal_path = entry.path().join(".extraction_journal");
                    let should_clean = if journal_path.exists() {
                        // 尝试读取 journal 判断任务是否仍在运行
                        match fs::read_to_string(&journal_path).await {
                            Ok(content) => {
                                if let Ok(journal) =
                                    serde_json::from_str::<serde_json::Value>(&content)
                                {
                                    // 检查 owner_pid：如果进程仍存活，任务可能在运行
                                    if let Some(owner_pid) =
                                        journal.get("owner_pid").and_then(|v| v.as_u64())
                                    {
                                        let pid = owner_pid as u32;
                                        if pid == std::process::id() {
                                            // 同一进程：journal 不是孤儿
                                            tracing::debug!(
                                                session_dir = %entry.path().display(),
                                                "[dream] journal owner 是当前进程，保留 marker"
                                            );
                                            false
                                        } else {
                                            // Unix: 检查 /proc/{pid} 是否存在
                                            #[cfg(unix)]
                                            {
                                                if std::path::Path::new(&format!("/proc/{}", pid))
                                                    .exists()
                                                {
                                                    tracing::debug!(
                                                        session_dir = %entry.path().display(),
                                                        pid,
                                                        "[dream] journal owner 进程仍存活，保留 marker"
                                                    );
                                                    false
                                                } else {
                                                    // 进程已死，使用 started_at + 3x 阈值做最终判断
                                                    is_journal_started_at_expired(
                                                        &journal,
                                                        EXTRACTION_STALE_THRESHOLD_SECS * 3,
                                                    )
                                                }
                                            }
                                            #[cfg(not(unix))]
                                            {
                                                // Windows 下无法无依赖检查 PID 存活，
                                                // 使用 started_at + 3x 阈值做保守判断
                                                let _ = pid;
                                                is_journal_started_at_expired(
                                                    &journal,
                                                    EXTRACTION_STALE_THRESHOLD_SECS * 3,
                                                )
                                            }
                                        }
                                    } else {
                                        // 无 owner_pid（旧格式），使用 started_at + 3x 阈值
                                        is_journal_started_at_expired(
                                            &journal,
                                            EXTRACTION_STALE_THRESHOLD_SECS * 3,
                                        )
                                    }
                                } else {
                                    true // 无法解析 JSON，清理
                                }
                            }
                            Err(_) => true, // 无法读取 journal，清理
                        }
                    } else {
                        true // 无 journal，清理
                    };

                    if should_clean {
                        tracing::warn!(
                            session_dir = %entry.path().display(),
                            "[dream] 清理过期的 extraction pending marker 和 journal（journal 确认可清理）"
                        );
                        let _ = fs::remove_file(&pending_marker).await;
                        if journal_path.exists() {
                            let _ = fs::remove_file(&journal_path).await;
                        }
                        // 清理后继续读取当前 memory.md
                    } else {
                        tracing::debug!(
                            session_dir = %entry.path().display(),
                            "[dream] marker 虽然过期但 journal 显示任务仍在运行，跳过"
                        );
                        continue;
                    }
                } else {
                    tracing::debug!(
                        session_dir = %entry.path().display(),
                        "[dream] 跳过正在提取 Session Memory 的会话（marker 未过期）"
                    );
                    continue;
                }
            }
            let memory_file = entry.path().join("memory.md");
            if fs::try_exists(&memory_file).await? {
                if let Ok(metadata) = fs::metadata(&memory_file).await {
                    if let Ok(modified) = metadata.modified() {
                        session_files.push((memory_file, modified));
                    }
                }
            }
        }

        // 按修改时间降序排序（最新的优先）
        session_files.sort_by_key(|b| std::cmp::Reverse(b.1));

        // 限制处理数量
        let files_to_process = session_files.iter().take(MAX_SESSIONS_TO_PROCESS);

        for (memory_file, modified_time) in files_to_process {
            match fs::read_to_string(memory_file).await {
                Ok(content) => {
                    // 提取信号
                    let signal = self.extract_signals_from_memory(&content, *modified_time);
                    if !signal.is_empty() {
                        tracing::trace!(
                            path = %memory_file.display(),
                            signal_count = signal.len(),
                            "extracted signals from session memory"
                        );
                        signals.extend(signal);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        path = %memory_file.display(),
                        error = %e,
                        "failed to read session memory"
                    );
                }
            }
        }

        tracing::info!(
            total_signals = signals.len(),
            "[dream] Phase 2: Gathered {} signals",
            signals.len()
        );

        Ok(signals)
    }

    /// 从 session memory 内容中提取信号
    ///
    /// 支持一级标题 (`# `) 和二级标题 (`## `)，与 Session Memory 10-section 模板兼容。
    /// 同时向后兼容旧格式（仅含二级标题的文件）。
    fn extract_signals_from_memory(
        &self,
        content: &str,
        modified_time: SystemTime,
    ) -> Vec<GatheredSignal> {
        let mut signals = Vec::new();

        // 按 markdown 标题分割：支持 `# `（一级）和 `## `（二级）
        // 使用行扫描方式，识别每行开头的 heading marker
        let sections = split_by_markdown_headings(content);

        for section in &sections {
            let section = section.trim();
            if section.is_empty() {
                continue;
            }

            // 提取章节标题（第一行，去除 heading marker）
            let title_end = section.find('\n').unwrap_or(section.len());
            let raw_title = section[..title_end].trim();
            // 去除 heading marker（`# ` 或 `## `）
            let title = raw_title
                .trim_start_matches("# ")
                .trim_start_matches("## ")
                .trim();

            // 提取章节内容（跳过标题行和换行符）
            let section_content = if title_end < section.len() {
                section[title_end..].trim()
            } else {
                ""
            };

            if !section_content.is_empty() && section_content != format!("*{}*", title).as_str() {
                // 计算内容的重要性分数
                let importance = self.calculate_signal_importance(title, section_content);

                if importance > 0 {
                    signals.push(GatheredSignal {
                        title: title.to_string(),
                        content: section_content.to_string(),
                        importance,
                        source_time: modified_time,
                    });
                }
            }
        }

        signals
    }

    /// 计算信号的重要性分数 (0-10)
    ///
    /// 基于 Session Memory 10-section 模板的实际章节标题：
    /// - Session Title, Current State, Task specification, Files and Functions,
    ///   Workflow, Errors & Corrections, Codebase and System Documentation,
    ///   Learnings, Key results, Worklog
    fn calculate_signal_importance(&self, title: &str, content: &str) -> u8 {
        // 归一化标题用于匹配（trim + 大小写不敏感）
        let normalized = title.trim().to_lowercase();

        // 高重要性章节：直接影响后续工作的关键信息
        let high_priority = [
            "current state",
            "errors & corrections",
            "errors and corrections",
        ];
        // 中重要性章节：任务定义、文件、关键结果
        let medium_priority = [
            "task specification",
            "files and functions",
            "key results",
            "decisions & preferences",
            "artifacts & files",
        ];
        // 低重要性章节：工作流和工作日志
        let low_priority = ["workflow", "worklog", "work log"];

        if high_priority.iter().any(|t| normalized.contains(t)) {
            8
        } else if medium_priority.iter().any(|t| normalized.contains(t)) {
            5
        } else if low_priority.iter().any(|t| normalized.contains(t)) {
            2
        } else {
            // 根据内容长度判断
            let content_len = content.len();
            if content_len > 500 {
                4
            } else if content_len > 200 {
                3
            } else {
                1
            }
        }
    }

    /// 阶段 3: 整合知识
    async fn consolidate(&self, signals: &[GatheredSignal]) -> Result<(), DreamError> {
        tracing::debug!(
            signal_count = signals.len(),
            "[dream] Phase 3: Consolidating knowledge"
        );

        // 检查 provider_pool
        let provider_pool = self
            .provider_pool
            .as_ref()
            .ok_or(DreamError::NoProviderPool)?;

        // 构建整合提示（包含收集的信号）
        let memory_dir = self.config_dir.join("memory");
        fs::create_dir_all(&memory_dir).await?;
        let prompt = self.build_consolidation_prompt(&memory_dir, signals);

        // 创建工具权限检查
        let can_use_tool = create_dream_can_use_tool(&memory_dir);

        // 创建 CacheSafeParams（使用默认系统提示）
        let cache_safe_params = CacheSafeParams::default();

        // 熔断器检查：如果熔断器打开，跳过整合
        let cb = get_dream_circuit_breaker();
        if !cb.allow() {
            tracing::warn!("[dream] Circuit breaker is open, skipping consolidation");
            return Err(DreamError::CircuitBreakerOpen);
        }

        // 运行 Forked Agent 进行整合
        // 使用 Builder 模式构建参数
        let params = ForkedAgentParams::builder()
            .provider_pool(provider_pool.clone())
            .prompt_messages(vec![ChatMessage::user(&prompt)])
            .cache_safe_params(cache_safe_params)
            .can_use_tool(can_use_tool)
            // 将执行层也限制在记忆目录内，避免无 working_dir 时接受任意绝对路径。
            .working_dir(memory_dir)
            .query_source("auto_dream")
            .fork_label("auto_dream")
            .max_turns(10)
            .skip_transcript(true)
            .tool_schemas(build_forked_tool_schemas(&["exec".to_string()]))
            .build()
            .map_err(|e| {
                DreamError::ConsolidationFailed(format!("Failed to build params: {}", e))
            })?;

        let result = run_forked_agent(params).await;

        match result {
            Ok(agent_result) => {
                // 检查工具调用失败和 max_turns 截断：二者都不应视为 consolidation 成功。
                if let Err(reason) = validate_dream_agent_result(&agent_result) {
                    // 熔断器记录失败
                    cb.record_failure();

                    tracing::error!(
                        session_ids = ?agent_result.files_modified,
                        response = ?agent_result.final_content,
                        truncated = agent_result.truncated,
                        had_tool_error = agent_result.had_tool_error,
                        reason = %reason,
                        "[dream] Forked Agent did not complete consolidation"
                    );
                    return Err(DreamError::ConsolidationFailed(reason));
                }

                // 熔断器记录成功
                cb.record_success();

                tracing::info!(
                    input_tokens = agent_result.total_usage.input_tokens,
                    output_tokens = agent_result.total_usage.output_tokens,
                    cache_hit_rate = agent_result.total_usage.cache_hit_rate(),
                    "[dream] Forked Agent completed"
                );
                Ok(())
            }
            Err(e) => {
                // 熔断器记录失败
                cb.record_failure();

                tracing::error!(error = %e, "[dream] Forked Agent failed");
                Err(DreamError::ConsolidationFailed(format!("{}", e)))
            }
        }
    }

    /// 构建整合提示
    fn build_consolidation_prompt(&self, memory_dir: &Path, signals: &[GatheredSignal]) -> String {
        // 按重要性排序信号
        let mut sorted_signals = signals.to_vec();
        sorted_signals.sort_by_key(|b| std::cmp::Reverse(b.importance));

        // 构建信号摘要
        let signals_section = if sorted_signals.is_empty() {
            "无新信号需要整合。\n".to_string()
        } else {
            let mut section = String::new();
            section.push_str("以下是从最近会话中收集的新信号（按重要性排序）：\n\n");

            for signal in sorted_signals.iter().take(20) {
                // 限制最多20个信号
                section.push_str(&format!(
                    "### {} (重要性: {}/10)\n{}\n\n",
                    signal.title, signal.importance, signal.content
                ));
            }

            section
        };

        format!(
            r#"# Dream: Memory Consolidation

## 任务
对记忆文件进行回顾、整理、更新和索引优化。

## 记忆目录
{}

## 收集的新信号
{}

## 执行阶段

### Phase 1 — Orient (定位)
- `ls` 记忆目录查看现有内容
- 读取入口文件理解当前索引
- 浏览现有主题文件避免重复创建

### Phase 2 — Gather recent signal (收集新信号)
优先级排序：
1. Daily logs（日志流）
2. 已过时的记忆（需要修正）
3. Transcript search（特定上下文搜索）

### Phase 3 — Consolidate (整合)
- 合并新信号到现有主题文件
- 将相对日期转换为绝对日期
- 删除被证伪的事实
- 更新过时信息

### Phase 4 — Prune and index (修剪和索引)
- 更新入口文件（保持 < 100 行, < 25KB）
- 移除过时指针
- 添加新指针
- 优化索引结构

## 工具限制
- 只读工具: read_file/list_dir/grep/glob 必须使用上方记忆目录内的显式路径
- Shell/Exec: 默认不提供；如被调用，也必须限定在记忆目录内
- Edit/Write: 仅限记忆目录内

## 注意事项
- 不要删除现有记忆，除非确认过时
- 合并相似条目
- 保持信息密度
"#,
            memory_dir.display(),
            signals_section
        )
    }

    /// 阶段 4: 修剪索引
    async fn prune(&self) -> Result<DreamStats, DreamError> {
        tracing::debug!("[dream] Phase 4: Pruning indexes");

        // 清理过期的 session memory 文件
        self.prune_expired_session_memories().await
    }

    /// 清理过期的 session memory 文件
    async fn prune_expired_session_memories(&self) -> Result<DreamStats, DreamError> {
        let sessions_dir = self.config_dir.join("sessions");

        if !fs::try_exists(&sessions_dir).await? {
            return Ok(DreamStats::default());
        }

        let expiry_threshold = SESSION_MEMORY_EXPIRY_DAYS * 24 * 3600; // 转换为秒
        let active_threshold = 3600; // 1小时内更新视为活跃会话
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut entries = fs::read_dir(&sessions_dir).await?;
        let mut pruned_count = 0;
        let mut skipped_active = 0;

        while let Some(entry) = entries.next_entry().await? {
            // 跳过非目录条目（如 .DS_Store）
            if entry.file_type().await.map(|t| !t.is_dir()).unwrap_or(true) {
                continue;
            }
            let session_dir = entry.path();

            // 检查是否为活跃会话
            if self
                .is_session_active(&session_dir, now, active_threshold)
                .await?
            {
                skipped_active += 1;
                continue;
            }

            // 检查目录修改时间
            if let Ok(metadata) = fs::metadata(&session_dir).await {
                if let Ok(modified) = metadata.modified() {
                    let modified_secs = modified
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    // 如果超过过期阈值，删除整个目录
                    if now - modified_secs > expiry_threshold {
                        tracing::trace!(
                            path = %session_dir.display(),
                            age_days = (now - modified_secs) / (24 * 3600),
                            "pruning expired session memory"
                        );
                        fs::remove_dir_all(&session_dir).await?;
                        pruned_count += 1;
                    }
                }
            }
        }

        tracing::info!(
            pruned_count,
            skipped_active,
            "[dream] Phase 4: Pruned {} expired session memories ({} active sessions skipped)",
            pruned_count,
            skipped_active
        );

        Ok(DreamStats {
            sessions_pruned: pruned_count,
            sessions_processed: pruned_count + skipped_active,
            ..Default::default()
        })
    }

    /// 检查会话是否仍在活跃运行
    ///
    /// 通过检查 `.active` 文件是否存在且最近更新来判断。
    /// 如果文件不存在或超过阈值时间未更新，则视为非活跃。
    async fn is_session_active(
        &self,
        session_dir: &Path,
        now: u64,
        active_threshold_secs: u64,
    ) -> Result<bool, DreamError> {
        let active_file = session_dir.join(".active");

        // 如果 .active 文件不存在，会话非活跃
        if !fs::try_exists(&active_file).await? {
            return Ok(false);
        }

        // 检查文件修改时间
        match fs::metadata(&active_file).await {
            Ok(metadata) => {
                match metadata.modified() {
                    Ok(modified) => {
                        let modified_secs = modified
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);

                        // 如果最近有更新，视为活跃
                        let is_active = now.saturating_sub(modified_secs) < active_threshold_secs;
                        Ok(is_active)
                    }
                    Err(_) => Ok(false),
                }
            }
            Err(_) => Ok(false),
        }
    }

    /// 扫描 memory 目录，获取文件状态
    ///
    /// 返回 (文件数量, 总字节数, 文件修改时间映射)
    async fn scan_memory_dir(&self, memory_dir: &Path) -> MemoryDirState {
        let mut file_count = 0;
        let mut total_bytes = 0u64;
        let mut file_mtimes: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();

        match fs::try_exists(memory_dir).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::debug!(path = %memory_dir.display(), "Memory directory does not exist");
                return MemoryDirState::default();
            }
            Err(e) => {
                tracing::debug!(path = %memory_dir.display(), error = %e, "Failed to check memory directory existence");
                return MemoryDirState::default();
            }
        }

        match fs::read_dir(memory_dir).await {
            Ok(mut entries) => {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.extension().map(|e| e == "md").unwrap_or(false) {
                        file_count += 1;
                        if let Ok(metadata) = fs::metadata(&path).await {
                            total_bytes += metadata.len();
                            if let Ok(modified) = metadata.modified() {
                                let mtime = modified
                                    .duration_since(UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0);
                                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                    file_mtimes.insert(name.to_string(), mtime);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::debug!(path = %memory_dir.display(), error = %e, "Failed to read memory directory");
            }
        }

        MemoryDirState {
            file_count,
            total_bytes,
            file_mtimes,
        }
    }

    /// 计算前后 memory 目录的差异
    fn compute_memory_diff(&self, pre: &MemoryDirState, post: &MemoryDirState) -> DreamStats {
        let mut created = 0;
        let mut updated = 0;
        let mut deleted = 0;

        // 检查新增和更新
        for (name, post_mtime) in &post.file_mtimes {
            match pre.file_mtimes.get(name) {
                Some(pre_mtime) => {
                    // 文件已存在，检查是否更新
                    if post_mtime > pre_mtime {
                        updated += 1;
                    }
                }
                None => {
                    // 新文件
                    created += 1;
                }
            }
        }

        // 检查删除
        for name in pre.file_mtimes.keys() {
            if !post.file_mtimes.contains_key(name) {
                deleted += 1;
            }
        }

        DreamStats {
            memories_created: created,
            memories_updated: updated,
            memories_deleted: deleted,
            ..Default::default()
        }
    }

    /// 增加会话计数
    pub fn increment_session_count(&mut self) {
        self.state.increment_session_count();
    }

    /// 获取当前状态
    pub fn state(&self) -> &DreamState {
        &self.state
    }
}

/// 检查锁的有效性（独立函数，供 check_gates 和 acquire_lock 复用）
///
/// 返回 Ok(true) 表示锁仍有效（进程存活且未过期）
/// 返回 Ok(false) 表示锁已失效（进程已死或过期）
async fn check_lock_validity(lock_path: &Path) -> Result<bool, DreamError> {
    let content = fs::read_to_string(lock_path).await?;

    // 解析 PID:TIMESTAMP
    let parts: Vec<&str> = content.split(':').collect();
    if parts.len() != 2 {
        // 格式错误，锁无效
        return Ok(false);
    }

    // 检查时间戳是否过期
    let timestamp: u64 = parts[1].parse().unwrap_or(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age_hours = (now - timestamp) / 3600;

    if age_hours >= TIME_GATE_THRESHOLD_HOURS {
        // 锁已过期
        tracing::debug!(age_hours, "[dream] Lock expired");
        return Ok(false);
    }

    // 检查持有锁的进程是否仍在运行
    let pid: u32 = parts[0].parse().unwrap_or(0);
    if pid == 0 {
        return Ok(false);
    }

    // 跨平台进程存活检查
    let process_alive = is_process_alive(pid);

    Ok(process_alive)
}

/// 检查进程是否存活（独立函数，供 check_lock_validity 复用）
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    // Unix: 使用 kill(pid, 0) 检查进程是否存在
    // ESRCH 表示进程不存在
    // SAFETY: libc::kill(pid, 0) 是安全的 Unix 系统调用，仅查询而不修改任何进程状态。
    //         信号值 0 是特殊用途，不发送实际信号，仅检查进程是否存在。
    //         该调用在 pid 不存在时返回 -1 并设置 errno 为 ESRCH，不存在未定义行为风险。
    unsafe {
        let result = libc::kill(pid as i32, 0);
        result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}

/// 检查进程是否存活（独立函数，供 check_lock_validity 复用）
#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
    // Windows: 尝试打开进程
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION,
    };

    // SAFETY: 以下 Windows API 调用的安全性说明：
    // - OpenProcess: 使用 PROCESS_QUERY_INFORMATION 权限打开已命名进程，
    //   传入来自锁文件的 pid，不修改任何进程状态。返回 null 表示进程不存在。
    // - GetExitCodeProcess: 从有效进程中读取退出码，不修改任何状态。
    // - CloseHandle: 关闭已打开的句柄，标准资源清理。
    // 所有调用均符合 Windows API 安全规范，不涉及内存安全风险。
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }

        let mut exit_code: u32 = 0;
        let result = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);

        // STILL_ACTIVE (259) 表示进程仍在运行
        //
        // 已知限制：如果进程恰好以退出码 259 结束，会被误判为仍在运行。
        // 这在现实中极其罕见，因为：
        // 1. 259 不是常见的错误码
        // 2. 大多数程序使用 0 表示成功，非零值表示错误
        // 3. 即使发生误判，锁也会在 TIME_GATE_THRESHOLD_HOURS 小时后过期
        //
        // 如果需要更精确的检测，可以使用 WaitForSingleObject 等待 0 毫秒，
        // 但那会增加代码复杂性。
        result != 0 && exit_code == 259
    }
}

/// 检查进程是否存活 (非 Unix 非 Windows 平台的保守实现)
#[cfg(not(any(unix, windows)))]
fn is_process_alive(_pid: u32) -> bool {
    // 保守策略：假设进程存活
    true
}

/// 梦境错误类型
#[derive(Debug, thiserror::Error)]
pub enum DreamError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Lock already acquired by another process")]
    LockAcquired,

    #[error("Consolidation failed: {0}")]
    ConsolidationFailed(String),

    #[error("No provider pool configured - call with_provider_pool() before dream()")]
    NoProviderPool,

    #[error("Consolidation timed out after {0}s")]
    Timeout(u64),

    #[error("Circuit breaker is open, dream consolidation blocked")]
    CircuitBreakerOpen,
}

/// 按 markdown 标题分割内容（支持 `# ` 和 `## `）
///
/// 返回 Vec，每个元素是一个 section（包含标题行和内容）。
/// 第一个标题之前的内容会被忽略，因为 session memory 模板以标题开头。
/// 维护 fenced code block 状态，避免误切 ``` 围栏内的 `#` 标题。
fn split_by_markdown_headings(content: &str) -> Vec<&str> {
    let mut sections = Vec::new();
    let mut section_start: Option<usize> = None;
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut pos = 0;
    // 追踪 fenced code block 状态，避免误切围栏内的 `#` 标题
    let mut in_fenced_code = false;
    // 记录当前 fence 的长度（3 或更多反引号），只有匹配长度的 fence 才能关闭
    let mut fence_len: usize = 0;

    while pos < len {
        // pos 始终在行开头（或 pos=0）
        // 查找当前行的结束位置
        let line_end = bytes[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| pos + p)
            .unwrap_or(len);

        // 检查当前行是否是 fence 开关（``` 或 ~~~）
        let line_bytes = &bytes[pos..line_end];
        let line = std::str::from_utf8(line_bytes).unwrap_or("");
        let trimmed = line.trim();

        // 检测 fence 行：行首（允许最多 3 个空格缩进）由 >=3 个 ` 或 ~ 组成
        if !in_fenced_code {
            // 尝试匹配 fence 开始
            let fence_char = if trimmed.starts_with("```") {
                b'`'
            } else if trimmed.starts_with("~~~") {
                b'~'
            } else {
                0
            };
            if fence_char != 0 {
                let fl = trimmed.bytes().take_while(|&c| c == fence_char).count();
                // fence 后面必须跟空白或行尾才算有效 fence
                if fl >= 3
                    && trimmed
                        .as_bytes()
                        .get(fl)
                        .copied()
                        .is_none_or(|c| c.is_ascii_whitespace())
                {
                    in_fenced_code = true;
                    fence_len = fl;
                }
            }
        } else {
            // 尝试匹配 fence 结束：只有相同字符、相同或更长长度才能关闭
            let fence_char = if trimmed.starts_with(&"`".repeat(fence_len)) {
                b'`'
            } else if trimmed.starts_with(&"~".repeat(fence_len)) {
                b'~'
            } else {
                0
            };
            if fence_char != 0 {
                let fl = trimmed.bytes().take_while(|&c| c == fence_char).count();
                if fl >= fence_len
                    && trimmed
                        .as_bytes()
                        .get(fl)
                        .copied()
                        .is_none_or(|c| c.is_ascii_whitespace())
                {
                    in_fenced_code = false;
                    fence_len = 0;
                }
            }
        }

        // 检查当前行是否是标题（仅在非 fenced code block 内判断）
        if !in_fenced_code {
            let is_h1 = trimmed.starts_with("# ") && !trimmed.starts_with("## ");
            let is_h2 = trimmed.starts_with("## ") && !trimmed.starts_with("### ");

            if is_h1 || is_h2 {
                // 结束前一个 section
                if let Some(start) = section_start {
                    let end = if pos > 0 && bytes[pos - 1] == b'\n' {
                        pos - 1 // 去掉前导换行符
                    } else {
                        pos
                    };
                    if end > start {
                        sections.push(&content[start..end]);
                    }
                }
                section_start = Some(pos);
            }
        }

        // 移动到下一行
        pos = if line_end < len { line_end + 1 } else { len };
    }

    // 处理最后一个 section
    if let Some(start) = section_start {
        if start < len {
            let remaining = content[start..].trim_end();
            if !remaining.is_empty() {
                sections.push(&content[start..]);
            }
        }
    } else if !content.trim().is_empty() {
        // 没有找到任何标题，整个内容作为一个 section（向后兼容旧格式）
        sections.push(content);
    }

    sections
}

/// 检查 journal 的 started_at 字段是否已超过指定阈值
///
/// 当 owner 进程已死或无法判断时，使用 started_at 时间做最终过期判断。
/// 阈值建议使用 3x 正常 stale 阈值（与 agent 侧 is_journal_stale 一致）。
fn is_journal_started_at_expired(journal: &serde_json::Value, threshold_secs: u64) -> bool {
    let started_at_str = journal
        .get("started_at")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let started_at = match chrono::DateTime::parse_from_rfc3339(started_at_str) {
        Ok(dt) => dt,
        Err(_) => return true, // 无法解析视为过期
    };
    let elapsed = chrono::Utc::now()
        .signed_duration_since(started_at.with_timezone(&chrono::Utc))
        .num_seconds();
    match elapsed {
        e if e >= 0 => (e as u64) >= threshold_secs,
        _ => false, // 未来时间戳，不视为过期
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dream_state_default() {
        let state = DreamState::default();
        assert!(state.last_consolidation_time.is_none());
        assert_eq!(state.current_session_count, 0);
        assert!(!state.is_consolidating);
        assert!(state.consolidating_started_at.is_none());
    }

    #[test]
    fn test_dream_state_increment() {
        let mut state = DreamState::default();
        state.increment_session_count();
        assert_eq!(state.current_session_count, 1);
    }

    #[test]
    fn test_noop_dream_advances_session_cursor_without_incrementing_count() {
        let mut state = DreamState {
            last_consolidation_time: Some(1),
            last_session_count: 5,
            current_session_count: 10,
            consolidation_count: 3,
            is_consolidating: false,
            consolidating_started_at: None,
        };
        let stats = DreamStats::default();

        apply_successful_dream_state(&mut state, &stats, 10, 99);

        assert_eq!(state.last_consolidation_time, Some(99));
        assert_eq!(state.last_session_count, 10);
        assert_eq!(state.consolidation_count, 3);
    }

    #[test]
    fn test_changed_dream_advances_session_cursor_and_count() {
        let mut state = DreamState {
            last_consolidation_time: Some(1),
            last_session_count: 5,
            current_session_count: 10,
            consolidation_count: 3,
            is_consolidating: false,
            consolidating_started_at: None,
        };
        let stats = DreamStats {
            memories_updated: 1,
            ..DreamStats::default()
        };

        apply_successful_dream_state(&mut state, &stats, 10, 99);

        assert_eq!(state.last_consolidation_time, Some(99));
        assert_eq!(state.last_session_count, 10);
        assert_eq!(state.consolidation_count, 4);
    }

    #[test]
    fn test_truncated_forked_agent_result_is_consolidation_failure() {
        let agent_result = blockcell_agent::forked::ForkedAgentResult {
            messages: vec![],
            total_usage: blockcell_core::UsageMetrics::default(),
            files_modified: vec![],
            final_content: Some("still working".to_string()),
            truncated: true,
            had_tool_error: false,
        };

        let err = validate_dream_agent_result(&agent_result).expect_err("truncated must fail");

        assert!(err.contains("truncated"));
    }

    #[test]
    fn test_completed_forked_agent_result_is_consolidation_success() {
        let agent_result = blockcell_agent::forked::ForkedAgentResult {
            messages: vec![],
            total_usage: blockcell_core::UsageMetrics::default(),
            files_modified: vec![],
            final_content: Some("done".to_string()),
            truncated: false,
            had_tool_error: false,
        };

        assert!(validate_dream_agent_result(&agent_result).is_ok());
    }

    #[tokio::test]
    async fn test_check_gates_time_failed() {
        let mut state = DreamState {
            last_consolidation_time: Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            ),
            last_session_count: 0,
            current_session_count: 10,
            consolidation_count: 1,
            is_consolidating: false,
            consolidating_started_at: None,
        };

        let result = check_gates(
            &mut state,
            Path::new("/config"),
            &ConsolidatorConfig::default(),
        )
        .await;
        assert_eq!(result, GateCheckResult::TimeGateFailed);
    }

    #[tokio::test]
    async fn test_check_gates_session_failed() {
        let mut state = DreamState {
            last_consolidation_time: Some(0), // 很久以前
            last_session_count: 0,
            current_session_count: 3, // 少于阈值 5
            consolidation_count: 1,
            is_consolidating: false,
            consolidating_started_at: None,
        };

        let result = check_gates(
            &mut state,
            Path::new("/config"),
            &ConsolidatorConfig::default(),
        )
        .await;
        assert_eq!(result, GateCheckResult::SessionGateFailed);
    }

    #[tokio::test]
    async fn test_check_gates_lock_failed_active() {
        // is_consolidating=true 且 consolidating_started_at 在阈值内 → 仍为活跃整合
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut state = DreamState {
            last_consolidation_time: Some(0),
            last_session_count: 0,
            current_session_count: 10,
            consolidation_count: 1,
            is_consolidating: true,              // 正在整合
            consolidating_started_at: Some(now), // 刚开始
        };

        let result = check_gates(
            &mut state,
            Path::new("/config"),
            &ConsolidatorConfig::default(),
        )
        .await;
        assert_eq!(result, GateCheckResult::LockGateFailed);
    }

    #[tokio::test]
    async fn test_check_gates_stale_consolidating_auto_recover() {
        // is_consolidating=true 但 consolidating_started_at 超过阈值 → 自动清除 stale 标记
        let stale_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(CONSOLIDATING_STALE_THRESHOLD_SECS + 100);
        let mut state = DreamState {
            last_consolidation_time: Some(0),
            last_session_count: 0,
            current_session_count: 10,
            consolidation_count: 1,
            is_consolidating: true,
            consolidating_started_at: Some(stale_time), // 超时
        };

        let result = check_gates(
            &mut state,
            Path::new("/config"),
            &ConsolidatorConfig::default(),
        )
        .await;
        // stale 标记被清除后，应继续检查时间和会话门控
        assert_eq!(result, GateCheckResult::Passed);
        // 内存中状态已清除
        assert!(!state.is_consolidating);
        assert!(state.consolidating_started_at.is_none());
    }

    #[tokio::test]
    async fn test_check_gates_passed() {
        let mut state = DreamState {
            last_consolidation_time: Some(0), // 很久以前
            last_session_count: 0,
            current_session_count: 10, // 超过阈值 5
            consolidation_count: 1,
            is_consolidating: false,
            consolidating_started_at: None,
        };

        let result = check_gates(
            &mut state,
            Path::new("/config"),
            &ConsolidatorConfig::default(),
        )
        .await;
        assert_eq!(result, GateCheckResult::Passed);
    }

    // ========== 核心路径测试 ==========

    #[test]
    fn test_gathered_signal_creation() {
        use std::time::SystemTime;

        let signal = GatheredSignal {
            title: "User Preferences".to_string(),
            content: "User prefers dark mode".to_string(),
            importance: 8,
            source_time: SystemTime::now(),
        };

        assert_eq!(signal.title, "User Preferences");
        assert_eq!(signal.importance, 8);
    }

    #[test]
    fn test_dream_state_serialization() {
        let state = DreamState {
            last_consolidation_time: Some(1234567890),
            last_session_count: 5,
            current_session_count: 10,
            consolidation_count: 3,
            is_consolidating: false,
            consolidating_started_at: None,
        };

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: DreamState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.last_consolidation_time, Some(1234567890));
        assert_eq!(deserialized.current_session_count, 10);
    }

    #[test]
    fn test_gate_check_result_variants() {
        // 确保所有变体都能正确创建和比较
        assert_eq!(
            GateCheckResult::TimeGateFailed,
            GateCheckResult::TimeGateFailed
        );
        assert_eq!(
            GateCheckResult::SessionGateFailed,
            GateCheckResult::SessionGateFailed
        );
        assert_eq!(
            GateCheckResult::LockGateFailed,
            GateCheckResult::LockGateFailed
        );
        assert_eq!(GateCheckResult::Passed, GateCheckResult::Passed);
    }

    #[test]
    fn test_dream_config_defaults() {
        assert_eq!(TIME_GATE_THRESHOLD_HOURS, 24);
        assert_eq!(SESSION_GATE_THRESHOLD, 5);
        assert_eq!(SESSION_MEMORY_EXPIRY_DAYS, 7);
        assert_eq!(MAX_SESSIONS_TO_PROCESS, 10);
        assert_eq!(CONSOLIDATING_STALE_THRESHOLD_SECS, 3600);
    }

    /// 测试：DreamState 与 agent 侧 DreamStateData 的 JSON schema 一致性
    ///
    /// 验证两个独立定义的结构体序列化/反序列化结果完全一致，
    /// 防止字段名、类型或 serde 属性不匹配导致跨 crate 数据丢失。
    /// 长期方案：将共享结构体移至 blockcell-core crate。
    #[test]
    fn test_dream_state_schema_consistency_with_agent_side() {
        use blockcell_agent::dream_state::DreamStateData;

        // 构造一个包含所有字段的完整实例
        let scheduler_state = DreamState {
            last_consolidation_time: Some(1234567890),
            last_session_count: 10,
            current_session_count: 15,
            consolidation_count: 3,
            is_consolidating: true,
            consolidating_started_at: Some(1234567800),
        };

        // 序列化 scheduler 侧结构体
        let scheduler_json = serde_json::to_value(&scheduler_state).unwrap();

        // 用 agent 侧结构体反序列化
        let agent_state: DreamStateData = serde_json::from_value(scheduler_json.clone()).unwrap();

        // 验证所有字段值一致
        assert_eq!(agent_state.last_consolidation_time, Some(1234567890u64));
        assert_eq!(agent_state.last_session_count, 10);
        assert_eq!(agent_state.current_session_count, 15);
        assert_eq!(agent_state.consolidation_count, 3);
        assert!(agent_state.is_consolidating);
        assert_eq!(agent_state.consolidating_started_at, Some(1234567800u64));

        // 反向：用 agent 侧结构体序列化，scheduler 侧反序列化
        let agent_json = serde_json::to_value(&agent_state).unwrap();
        let restored: DreamState = serde_json::from_value(agent_json.clone()).unwrap();
        assert_eq!(restored.last_consolidation_time, Some(1234567890));
        assert_eq!(restored.last_session_count, 10);
        assert_eq!(restored.current_session_count, 15);
        assert_eq!(restored.consolidation_count, 3);
        assert!(restored.is_consolidating);
        assert_eq!(restored.consolidating_started_at, Some(1234567800));

        // 验证 JSON key 集合完全一致
        let scheduler_keys: std::collections::BTreeSet<String> = scheduler_json
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let agent_keys: std::collections::BTreeSet<String> =
            agent_json.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            scheduler_keys, agent_keys,
            "scheduler 和 agent 侧 DreamState 的 JSON key 集合不一致"
        );
    }
}
