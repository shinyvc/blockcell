//! 提取游标管理
//!
//! 管理每种记忆类型的提取进度游标。
//!
//! ## 三重冷却机制
//!
//! 1. **消息计数冷却**: 距离上次提取需要经过一定数量的消息
//! 2. **时间冷却**: 距离上次提取需要经过一定时间
//! 3. **内容变化检测**: 消息内容需要有实质性变化
//!
//! ## 时间测量的安全性
//!
//! 使用 `Instant` (monotonic clock) 替代 `SystemTime` 进行时间差计算，
//! 避免系统时钟调整（NTP 同步、手动修改、时区变化等）导致的问题。

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::fs;
use uuid::Uuid;

use super::memory_type::MemoryType;

/// 进程内游标文件锁，防止同一进程内并发 merge_and_save 丢更新。
///
/// 每个 cursor 文件路径对应一个 Arc<Mutex<()>>，确保 read-merge-write 在进程内串行化。
/// 使用 Arc 以便 clone 后释放外层 HashMap 锁，避免生命周期冲突。
/// 跨进程并发由唯一临时文件名 + 原子 rename 自然处理。
static CURSOR_FILE_LOCKS: Lazy<std::sync::Mutex<HashMap<PathBuf, Arc<std::sync::Mutex<()>>>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

/// 跨进程文件锁（RAII）
///
/// 使用 `OpenOptions::new().create_new(true)` 原子创建锁文件，
/// 确保同一时刻只有一个进程能持有锁。
///
/// - `create_new` 在所有平台上都是原子的：文件不存在时创建，已存在时返回错误
/// - 锁文件内容为持有锁的 PID，便于诊断残留锁
/// - Drop 时删除锁文件，释放锁
/// - 如果锁文件已存在，短暂等待后重试
struct CrossProcessLock {
    lock_path: PathBuf,
}

impl CrossProcessLock {
    /// 最大重试次数
    const MAX_RETRIES: usize = 30;
    /// 重试间隔（毫秒）
    const RETRY_INTERVAL_MS: u64 = 200;

    /// 获取跨进程锁
    ///
    /// 尝试创建锁文件，若已存在则等待并重试。
    /// 只有当锁文件中记录的 PID 已不存在时，才清理残留锁。
    /// 活进程持有的锁不会被强制删除，避免中断合法的 read-merge-write。
    fn acquire(lock_path: &Path) -> std::io::Result<Self> {
        for attempt in 0..Self::MAX_RETRIES {
            match Self::try_create_lock(lock_path) {
                Ok(()) => {
                    return Ok(Self {
                        lock_path: lock_path.to_path_buf(),
                    })
                }
                Err(_) => {
                    // 锁文件已存在，检查持有进程是否已退出
                    if Self::is_holder_dead(lock_path) {
                        tracing::debug!(
                            attempt,
                            path = %lock_path.display(),
                            "[cursor] 锁文件持有进程已退出，清理残留锁"
                        );
                        let _ = std::fs::remove_file(lock_path);
                        // 立即重试，不等待
                        continue;
                    }
                    // 活进程持锁，等待后重试
                    if attempt < Self::MAX_RETRIES - 1 {
                        std::thread::sleep(std::time::Duration::from_millis(
                            Self::RETRY_INTERVAL_MS,
                        ));
                    }
                }
            }
        }

        // 重试耗尽且锁持有进程仍存活，返回错误而非强制删除
        tracing::warn!(
            path = %lock_path.display(),
            "[cursor] 跨进程锁获取超时，锁持有进程仍存活"
        );
        Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "跨进程锁获取超时: 另一个进程仍在持有锁 ({})",
                lock_path.display()
            ),
        ))
    }

    /// 尝试原子创建锁文件
    fn try_create_lock(lock_path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)?;
        // 写入 PID 便于诊断
        write!(file, "{}", std::process::id())?;
        Ok(())
    }

    /// 检查锁文件持有进程是否已退出
    ///
    /// 仅通过 PID 存活检测判断：PID 不存在 → 持有进程已退出，锁可安全清理。
    /// 不使用时间判断，因为活进程持锁时间长短不能说明锁是残留的。
    fn is_holder_dead(lock_path: &Path) -> bool {
        if let Ok(content) = std::fs::read_to_string(lock_path) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                return !is_pid_alive(pid);
            }
        }
        // 无法读取 PID，保守地认为进程仍存活
        false
    }
}

impl Drop for CrossProcessLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// 检查指定 PID 的进程是否仍存活
fn is_pid_alive(pid: u32) -> bool {
    use std::process::Command;

    #[cfg(unix)]
    {
        // kill -0 <pid> 不发送信号，仅检查进程是否存在
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        // 使用 tasklist 检查进程是否存在
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        // 未知平台，假设进程仍存活（保守策略）
        let _ = (pid, Command::new(""));
        true
    }
}

/// 时间冷却阈值（秒）
///
/// 默认 5 分钟 = 300 秒
///
/// 仅用作 AutoMemoryConfig::default() 的回退值，
/// 运行时使用 Layer5Config.extraction_time_cooldown_secs
pub const TIME_COOLDOWN_SECS: u64 = 300;

/// 内容变化阈值（字符数）
///
/// 默认值 500，可通过 Layer5Config.content_change_threshold 配置。
/// 仅用作 AutoMemoryConfig::default() 的回退值，
/// 运行时使用 Layer5Config.content_change_threshold
pub const CONTENT_CHANGE_THRESHOLD: usize = 500;

/// 单个记忆类型的游标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionCursor {
    /// 记忆类型
    pub memory_type: MemoryType,
    /// 上次提取的消息 UUID
    pub last_extracted_uuid: Option<Uuid>,
    /// 上次提取时的消息数
    pub last_message_count: usize,
    /// 上次提取时间戳（秒，用于持久化）
    pub last_extraction_time: Option<u64>,
    /// 提取次数
    pub extraction_count: usize,
    /// 上次提取时的内容签名（用于检测内容变化）
    pub last_content_signature: Option<u64>,
    /// 上次提取时的内容长度（用于精确计算内容变化量）
    pub last_content_length: Option<usize>,
    /// 上次提取的 monotonic 时间点（不序列化，运行时使用）
    #[serde(skip)]
    pub last_extraction_instant: Option<Instant>,
}

impl ExtractionCursor {
    /// 创建新游标
    pub fn new(memory_type: MemoryType) -> Self {
        Self {
            memory_type,
            last_extracted_uuid: None,
            last_message_count: 0,
            last_extraction_time: None,
            extraction_count: 0,
            last_content_signature: None,
            last_content_length: None,
            last_extraction_instant: None,
        }
    }

    /// 检查是否需要提取（消息计数冷却）
    pub fn should_extract(&self, current_message_count: usize, cooldown: usize) -> bool {
        let messages_since_last = current_message_count.saturating_sub(self.last_message_count);
        messages_since_last >= cooldown
    }

    /// 检查是否满足时间冷却
    ///
    /// 使用 monotonic clock (`Instant`) 进行时间差计算，
    /// 不受系统时钟调整影响。
    ///
    /// 返回 true 表示时间冷却已满足（可以提取）
    pub fn check_time_cooldown(&self, cooldown_secs: u64) -> bool {
        // 优先使用 Instant (monotonic clock)
        if let Some(instant) = self.last_extraction_instant {
            let elapsed = instant.elapsed().as_secs();
            return elapsed >= cooldown_secs;
        }

        // 回退到 SystemTime（用于从持久化状态恢复的情况）
        let last_time = match self.last_extraction_time {
            Some(t) => t,
            None => return true, // 从未提取过，时间冷却通过
        };

        // 使用 match 替代 unwrap_or_default，以便记录警告
        let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_secs(),
            Err(e) => {
                // 系统时钟异常（罕见情况：嵌入式系统、NTP 同步错误等）
                // 使用 0 作为 fallback，可能导致时间冷却立即通过
                tracing::warn!(
                    error = %e,
                    "[cursor] System clock before Unix epoch, time cooldown check may be inaccurate"
                );
                0
            }
        };

        let elapsed = now.saturating_sub(last_time);
        elapsed >= cooldown_secs
    }

    /// 检查内容是否有实质性变化
    ///
    /// 通过计算内容签名来检测变化
    /// `content_change_threshold` 来自 Layer5Config.content_change_threshold
    pub fn check_content_change(
        &self,
        current_content: &str,
        content_change_threshold: usize,
    ) -> bool {
        let current_signature = compute_content_signature(current_content);

        match self.last_content_signature {
            Some(last_sig) => {
                if current_signature == last_sig {
                    return false; // 签名相同，内容未变
                }
                // 签名不同，检查内容长度变化量
                let last_len = self.last_content_length.unwrap_or(0);
                let content_delta = current_content.len().abs_diff(last_len);
                content_delta >= content_change_threshold
            }
            None => true, // 从未提取过，内容变化通过
        }
    }

    /// 检查内容是否有实质性变化（使用默认阈值）
    ///
    /// 便捷方法，使用 CONTENT_CHANGE_THRESHOLD 常量作为默认值
    pub fn check_content_change_default(&self, current_content: &str) -> bool {
        self.check_content_change(current_content, CONTENT_CHANGE_THRESHOLD)
    }

    /// 综合检查是否应该提取
    ///
    /// 三个条件：
    /// 1. 消息计数冷却
    /// 2. 时间冷却
    /// 3. 内容变化（可选，根据 need_content_change 参数）
    ///
    /// `content_change_threshold` 来自 Layer5Config.content_change_threshold
    pub fn should_extract_full(
        &self,
        current_message_count: usize,
        current_content: &str,
        message_cooldown: usize,
        time_cooldown_secs: u64,
        require_content_change: bool,
        content_change_threshold: usize,
    ) -> ExtractionDecision {
        // 1. 消息计数冷却
        let messages_since_last = current_message_count.saturating_sub(self.last_message_count);
        let message_cooldown_met = messages_since_last >= message_cooldown;

        if !message_cooldown_met {
            return ExtractionDecision::Wait {
                reason: ExtractionWaitReason::MessageCooldown {
                    current: messages_since_last,
                    required: message_cooldown,
                },
            };
        }

        // 2. 时间冷却
        let time_cooldown_met = self.check_time_cooldown(time_cooldown_secs);

        if !time_cooldown_met {
            let elapsed = if let Some(instant) = self.last_extraction_instant {
                instant.elapsed().as_secs()
            } else if let Some(last_time) = self.last_extraction_time {
                // 使用 match 替代 unwrap_or_default，以便记录警告
                let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => d.as_secs(),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "[cursor] System clock error in time cooldown calculation"
                        );
                        0
                    }
                };
                now.saturating_sub(last_time)
            } else {
                0
            };

            return ExtractionDecision::Wait {
                reason: ExtractionWaitReason::TimeCooldown {
                    elapsed_secs: elapsed,
                    required_secs: time_cooldown_secs,
                },
            };
        }

        // 3. 内容变化（可选）
        if require_content_change {
            let content_changed =
                self.check_content_change(current_content, content_change_threshold);
            if !content_changed {
                return ExtractionDecision::Wait {
                    reason: ExtractionWaitReason::NoContentChange,
                };
            }
        }

        ExtractionDecision::Proceed
    }

    /// 更新游标
    pub fn update(&mut self, message_uuid: Uuid, message_count: usize) {
        self.last_extracted_uuid = Some(message_uuid);
        self.last_message_count = message_count;
        // 使用 monotonic clock 记录时间
        self.last_extraction_instant = Some(Instant::now());
        // 同时记录 Unix 时间戳用于持久化
        // 使用 match 替代 unwrap_or_default，持久化时 0 值是可接受的 fallback
        self.last_extraction_time = Some(
            match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d.as_secs(),
                Err(e) => {
                    // 系统时钟异常，使用 0 作为 fallback（持久化时可接受）
                    tracing::warn!(
                        error = %e,
                        "[cursor] System clock error when updating cursor, using 0 as timestamp"
                    );
                    0
                }
            },
        );
        self.extraction_count += 1;
    }

    /// 更新游标（包含内容签名和长度）
    pub fn update_with_content(&mut self, message_uuid: Uuid, message_count: usize, content: &str) {
        self.update(message_uuid, message_count);
        self.last_content_signature = Some(compute_content_signature(content));
        self.last_content_length = Some(content.len());
    }
}

/// 提取决策
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionDecision {
    /// 可以进行提取
    Proceed,
    /// 需要等待
    Wait { reason: ExtractionWaitReason },
}

/// 等待原因
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionWaitReason {
    /// 消息计数冷却未满足
    MessageCooldown { current: usize, required: usize },
    /// 时间冷却未满足
    TimeCooldown {
        elapsed_secs: u64,
        required_secs: u64,
    },
    /// 内容无变化
    NoContentChange,
}

/// 计算内容签名
///
/// 使用简单的哈希算法来检测内容变化
fn compute_content_signature(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// 游标管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionCursorManager {
    /// 各记忆类型的游标
    cursors: HashMap<String, ExtractionCursor>,
    /// 游标文件路径
    cursor_file_path: PathBuf,
}

impl ExtractionCursorManager {
    /// 创建新的游标管理器
    pub fn new(config_dir: &Path) -> Self {
        let cursor_file_path = config_dir.join("memory").join(".cursors.json");

        Self {
            cursors: HashMap::new(),
            cursor_file_path,
        }
    }

    /// 加载游标状态
    pub async fn load(&mut self) -> std::io::Result<()> {
        if let Ok(content) = fs::read_to_string(&self.cursor_file_path).await {
            if let Ok(manager) = serde_json::from_str::<ExtractionCursorManager>(&content) {
                self.cursors = manager.cursors;
            } else {
                // JSON 解析失败，备份损坏文件后使用默认值
                tracing::warn!(
                    path = %self.cursor_file_path.display(),
                    "[cursor] Failed to parse cursor file, backing up and using defaults"
                );
                // 备份损坏的文件，避免数据永久丢失
                // Note: with_extension replaces only the last extension part.
                // For ".cursors.json", we want ".cursors.json.bak", so use "json.bak".
                let backup_path = self.cursor_file_path.with_extension("json.bak");
                if let Err(e) = fs::rename(&self.cursor_file_path, &backup_path).await {
                    tracing::warn!(
                        error = %e,
                        "[cursor] Failed to backup corrupted cursor file"
                    );
                }
            }
        }
        Ok(())
    }

    /// 保存游标状态（通过跨进程锁保护）
    ///
    /// 使用与 merge_and_save() 相同的进程内 Mutex + 跨进程锁文件保护，
    /// 先读取磁盘最新状态再写入，避免覆盖其他进程已写入的 cursor。
    pub async fn save(&self) -> std::io::Result<()> {
        // 获取该 cursor 文件路径对应的进程内锁
        let file_lock = {
            let mut locks = CURSOR_FILE_LOCKS
                .lock()
                .expect("[cursor] CURSOR_FILE_LOCKS 不应被 poison");
            locks
                .entry(self.cursor_file_path.clone())
                .or_insert_with(|| Arc::new(std::sync::Mutex::new(())))
                .clone()
        };

        let _guard = file_lock
            .lock()
            .expect("[cursor] cursor 文件锁不应被 poison");

        // 跨进程锁文件路径
        let lock_path = self.cursor_file_path.with_extension("lock");

        // 尝试获取跨进程锁
        let _lock_guard = CrossProcessLock::acquire(&lock_path)?;

        // 读取磁盘上的最新状态（其他进程可能已写入）
        let mut cursors = self.cursors.clone();
        if let Ok(content) = std::fs::read_to_string(&self.cursor_file_path) {
            if let Ok(disk_manager) = serde_json::from_str::<ExtractionCursorManager>(&content) {
                // 合并：磁盘上的条目优先，但保留内存中独有的条目
                for (key, value) in disk_manager.cursors {
                    cursors.entry(key).or_insert(value);
                }
            }
        }

        // 序列化合并后的状态
        let content = serde_json::to_string_pretty(&ExtractionCursorManager {
            cursors,
            cursor_file_path: self.cursor_file_path.clone(),
        })?;

        if let Some(parent) = self.cursor_file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        crate::fs_util::atomic_write(&self.cursor_file_path, content.as_bytes())?;

        Ok(())
    }

    /// 获取特定记忆类型的游标
    pub fn get_cursor(&self, memory_type: MemoryType) -> ExtractionCursor {
        self.cursors
            .get(memory_type.name())
            .cloned()
            .unwrap_or_else(|| ExtractionCursor::new(memory_type))
    }

    /// 更新游标
    pub fn update_cursor(&mut self, cursor: ExtractionCursor) {
        self.cursors
            .insert(cursor.memory_type.name().to_string(), cursor);
    }

    /// Merge a single cursor update with the latest on-disk state, then save.
    ///
    /// 使用进程内 Mutex + 跨进程锁文件保护 read-merge-write 周期：
    /// - 进程内 Mutex 串行化同进程的并发调用
    /// - 跨进程锁文件使用 `create_new` 原子创建，确保同一时刻只有一个进程
    ///   能进入 read-merge-write 周期
    /// - 锁文件路径: `<cursor_file>.lock`，内容为持有锁的 PID
    /// - 最多重试 30 次，每次间隔 200ms（总计约 6 秒）
    /// - 仅当锁持有进程已退出时才清理残留锁，活进程锁不会被强制删除
    pub async fn merge_and_save(&mut self, cursor: ExtractionCursor) -> std::io::Result<()> {
        // 获取该 cursor 文件路径对应的进程内锁
        let file_lock = {
            let mut locks = CURSOR_FILE_LOCKS
                .lock()
                .expect("[cursor] CURSOR_FILE_LOCKS 不应被 poison");
            locks
                .entry(self.cursor_file_path.clone())
                .or_insert_with(|| Arc::new(std::sync::Mutex::new(())))
                .clone()
        };

        let _guard = file_lock
            .lock()
            .expect("[cursor] cursor 文件锁不应被 poison");

        // 跨进程锁文件路径
        let lock_path = self.cursor_file_path.with_extension("lock");

        // 尝试获取跨进程锁
        let _lock_guard = CrossProcessLock::acquire(&lock_path)?;

        // 读取磁盘上的最新状态并合并
        if let Ok(content) = std::fs::read_to_string(&self.cursor_file_path) {
            if let Ok(disk_manager) = serde_json::from_str::<ExtractionCursorManager>(&content) {
                self.cursors = disk_manager.cursors;
            }
        }

        // 应用当前更新
        self.cursors
            .insert(cursor.memory_type.name().to_string(), cursor);

        // 写入
        let content = serde_json::to_string_pretty(&self)?;
        if let Some(parent) = self.cursor_file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        crate::fs_util::atomic_write(&self.cursor_file_path, content.as_bytes())?;

        Ok(())
    }

    /// 获取所有游标
    pub fn all_cursors(&self) -> Vec<ExtractionCursor> {
        MemoryType::all()
            .iter()
            .map(|mt| self.get_cursor(*mt))
            .collect()
    }

    /// 重置所有游标
    pub fn reset_all(&mut self) {
        self.cursors.clear();
        for mt in MemoryType::all() {
            self.cursors
                .insert(mt.name().to_string(), ExtractionCursor::new(mt));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extraction_cursor_new() {
        let cursor = ExtractionCursor::new(MemoryType::User);
        assert_eq!(cursor.memory_type, MemoryType::User);
        assert!(cursor.last_extracted_uuid.is_none());
        assert_eq!(cursor.last_message_count, 0);
    }

    #[test]
    fn test_extraction_cursor_should_extract() {
        let cursor = ExtractionCursor::new(MemoryType::User);

        // 初始状态，消息数不足
        assert!(!cursor.should_extract(3, 5));

        // 消息数足够
        assert!(cursor.should_extract(10, 5));
    }

    #[test]
    fn test_extraction_cursor_update() {
        let mut cursor = ExtractionCursor::new(MemoryType::User);
        let uuid = Uuid::new_v4();

        cursor.update(uuid, 15);

        assert_eq!(cursor.last_extracted_uuid, Some(uuid));
        assert_eq!(cursor.last_message_count, 15);
        assert!(cursor.last_extraction_time.is_some());
        assert_eq!(cursor.extraction_count, 1);
    }

    #[test]
    fn test_cursor_manager_new() {
        let manager = ExtractionCursorManager::new(Path::new("/config"));
        assert!(manager.cursors.is_empty());
    }

    #[test]
    fn test_cursor_manager_get_cursor() {
        let manager = ExtractionCursorManager::new(Path::new("/config"));

        // 未存储的游标会创建新的
        let cursor = manager.get_cursor(MemoryType::User);
        assert_eq!(cursor.memory_type, MemoryType::User);
    }

    #[test]
    fn test_cursor_manager_update_cursor() {
        let mut manager = ExtractionCursorManager::new(Path::new("/config"));

        let mut cursor = ExtractionCursor::new(MemoryType::User);
        cursor.update(Uuid::new_v4(), 10);

        manager.update_cursor(cursor.clone());

        let retrieved = manager.get_cursor(MemoryType::User);
        assert_eq!(retrieved.last_message_count, 10);
    }

    #[test]
    fn test_cursor_manager_all_cursors() {
        let manager = ExtractionCursorManager::new(Path::new("/config"));
        let cursors = manager.all_cursors();

        assert_eq!(cursors.len(), 4);
    }
}
