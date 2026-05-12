//! Session Memory 恢复机制
//!
//! 提供 Post-Compact 恢复和等待提取完成的功能。

use crate::memory_event;
use crate::token::estimate_tokens;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::time::{timeout, Duration};

/// 获取 Session Memory 目录路径
pub fn get_session_memory_dir(workspace_dir: &Path, session_id: &str) -> PathBuf {
    use blockcell_core::session_file_stem;
    workspace_dir
        .join("sessions")
        .join(session_file_stem(session_id))
}

/// 获取 Session Memory 文件路径
pub fn get_session_memory_path(workspace_dir: &Path, session_id: &str) -> PathBuf {
    get_session_memory_dir(workspace_dir, session_id).join("memory.md")
}

/// 等待 Session Memory 提取完成
///
/// 用于 Post-Compact 恢复前等待后台提取完成。
/// 使用 Layer3Config 中的超时参数作为默认值。
///
/// 注意：此便捷函数使用硬编码常量作为回退值。
/// 生产代码应使用 `wait_for_session_memory_extraction_with_timeout()` 并传入
/// 从 Layer3Config 获取的超时参数。
pub async fn wait_for_session_memory_extraction(
    memory_path: &Path,
    extraction_started_at: Option<std::time::Instant>,
    wait_timeout_ms: u64,
    stale_threshold_ms: u64,
) -> Result<(), std::io::Error> {
    wait_for_session_memory_extraction_with_timeout(
        memory_path,
        extraction_started_at,
        wait_timeout_ms,
        stale_threshold_ms,
    )
    .await
}

/// 等待 Session Memory 提取完成（带可配置超时）
///
/// 使用文件 mtime > extraction_start_time 作为完成信号，
/// 而非"文件非空"（因为模板文件在提取开始时就已创建且非空）。
pub async fn wait_for_session_memory_extraction_with_timeout(
    memory_path: &Path,
    extraction_started_at: Option<std::time::Instant>,
    wait_timeout_ms: u64,
    stale_threshold_ms: u64,
) -> Result<(), std::io::Error> {
    // 如果没有提取任务，直接返回
    let start_time = match extraction_started_at {
        Some(t) => t,
        None => return Ok(()),
    };

    // 将 Instant 转换为 SystemTime 用于 mtime 比较
    // Instant 没有直接到 SystemTime 的转换，所以我们记录当前偏移量
    let now_system = std::time::SystemTime::now();
    let now_instant = std::time::Instant::now();
    let extraction_start_system = now_system - (now_instant - start_time);

    // 计算剩余等待时间
    let elapsed = start_time.elapsed().as_millis() as u64;
    let remaining = wait_timeout_ms.saturating_sub(elapsed);

    if remaining == 0 {
        // 已超时，检查是否 stale
        if elapsed > stale_threshold_ms {
            tracing::warn!(
                elapsed_ms = elapsed,
                "[session_memory] extraction is stale, proceeding without waiting"
            );
            return Ok(());
        }
    }

    // 等待文件 mtime > extraction_start_system 或超时
    let result = timeout(
        Duration::from_millis(remaining),
        wait_for_mtime_after(memory_path, extraction_start_system),
    )
    .await;

    match result {
        Ok(Ok(())) => {
            tracing::info!("[session_memory] extraction completed successfully");
            Ok(())
        }
        Ok(Err(e)) => {
            // 内部 IO 错误（如轮询超时未检测到 mtime 更新）
            tracing::warn!(
                error = %e,
                "[session_memory] extraction wait failed internally, proceeding"
            );
            Err(e)
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = remaining,
                "[session_memory] wait timed out, proceeding"
            );
            Ok(())
        }
    }
}

/// 等待文件 mtime 大于 extraction_start_time（表示提取已写入新内容）
///
/// 使用 mtime 而非"文件非空"作为完成信号，因为模板文件在提取开始时
/// 就已经创建且非空，"文件非空"会误判提取已完成。
///
/// 内部轮询不设独立总时长上限，由外层 `tokio::time::timeout()` 控制实际截止时间。
/// 这样用户配置的 `wait_timeout_ms` 才能真正生效（如 30s/60s）。
async fn wait_for_mtime_after(
    path: &Path,
    extraction_start_system: std::time::SystemTime,
) -> Result<(), std::io::Error> {
    const POLL_INTERVAL_MS: u64 = 300;

    loop {
        if fs::try_exists(path).await? {
            let metadata = fs::metadata(path).await?;
            if let Ok(modified) = metadata.modified() {
                // 文件 mtime > 提取开始时间 → 提取已完成
                if modified > extraction_start_system {
                    return Ok(());
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

/// 获取 Session Memory 内容用于 Compact
///
/// 如果内容为空或不存在，返回模板。
pub async fn get_session_memory_content_for_compact(
    memory_path: &Path,
    template: &str,
    max_tokens: usize,
) -> Result<String, std::io::Error> {
    // 读取文件内容
    let content = match fs::read_to_string(memory_path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 文件不存在，返回模板
            return Ok(template.to_string());
        }
        Err(e) => return Err(e),
    };

    // 检查是否为空
    if super::template::is_session_memory_empty(&content) {
        return Ok(template.to_string());
    }

    // 检查是否需要截断
    let (truncated, was_truncated) =
        super::template::truncate_session_memory_for_compact(&content, max_tokens);

    if was_truncated {
        tracing::info!(
            original_tokens = estimate_tokens(&content),
            max_tokens = max_tokens,
            "[session_memory] truncated for compact"
        );
    }

    // 记录 Layer 3 加载事件
    let content_length = truncated.len();
    let line_count = truncated.lines().count() as u64;
    let section_count = truncated.matches("## ").count() as u64;
    memory_event!(layer3, loaded, content_length, line_count, section_count);

    Ok(truncated)
}

/// 创建 Session Memory 恢复上下文
///
/// 用于 Post-Compact 阶段恢复 Session Memory 信息。
///
/// ## 设计意图 (Layer 4 - Post-Compact 恢复)
/// 根据 7 层记忆系统设计文档，Post-Compact 阶段需要：
/// 1. 文件恢复 - 恢复最近读取的文件内容
/// 2. 技能恢复 - 恢复已加载的技能状态
/// 3. Session Memory 恢复 - 恢复会话摘要信息
///
/// 此结构体用于实现第 3 项。当前为预留接口，待后续集成。
#[allow(dead_code)]
pub struct SessionMemoryRecoveryContext {
    /// Session Memory 文件路径
    pub memory_path: PathBuf,
    /// 当前内容（已截断）
    pub content: String,
    /// 是否为模板（无实际内容）
    pub is_template: bool,
    /// 提取开始时间
    pub extraction_started_at: Option<std::time::Instant>,
}

impl SessionMemoryRecoveryContext {
    /// 创建恢复上下文
    ///
    /// 已集成到 Compact 流程：通过 `SessionMemoryRecoveryHook` 在 Post-Compact 阶段调用。
    #[allow(dead_code)]
    pub async fn create(
        workspace_dir: &Path,
        session_id: &str,
        template: &str,
        max_tokens: usize,
        extraction_started_at: Option<std::time::Instant>,
        extraction_wait_timeout_ms: u64,
        extraction_stale_threshold_ms: u64,
    ) -> Result<Self, std::io::Error> {
        let memory_path = get_session_memory_path(workspace_dir, session_id);

        // 等待提取完成（使用可配置超时）
        wait_for_session_memory_extraction_with_timeout(
            &memory_path,
            extraction_started_at,
            extraction_wait_timeout_ms,
            extraction_stale_threshold_ms,
        )
        .await?;

        // 获取内容
        let content =
            get_session_memory_content_for_compact(&memory_path, template, max_tokens).await?;

        // 判断是否为模板
        let is_template = content == template;

        Ok(Self {
            memory_path,
            content,
            is_template,
            extraction_started_at,
        })
    }

    /// 生成 Post-Compact 恢复消息
    ///
    /// 已集成到 `PostCompactResult::NeedRecovery`：当 Compact 完成后，
    /// 通过 `SessionMemoryRecoveryHook` 将恢复消息注入到对话历史中。
    #[allow(dead_code)]
    pub fn generate_recovery_message(&self) -> String {
        if self.is_template {
            // 无实际内容，提供简化恢复信息
            format!(
                "Session Memory file created at {} but contains no content yet.\n\
                 The session is fresh or no significant information accumulated.",
                self.memory_path.display()
            )
        } else {
            // 有内容，提供完整恢复信息
            format!(
                "## Session Memory Recovery\n\n\
                 Session Memory file: {}\n\n\
                 ```markdown\n{}\n```",
                self.memory_path.display(),
                self.content
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_get_session_memory_path() {
        let workspace = Path::new("/workspace");
        let path = get_session_memory_path(workspace, "test-session");
        // Check path components instead of string representation (platform-independent)
        assert!(path.ends_with("memory.md"));
        assert!(path.to_str().unwrap().contains("sessions"));
        assert!(path.to_str().unwrap().contains("test-session"));
    }

    #[test]
    fn test_get_session_memory_dir() {
        let workspace = Path::new("/workspace");
        let dir = get_session_memory_dir(workspace, "test-session");
        // Check path components instead of string representation (platform-independent)
        assert!(dir.to_str().unwrap().contains("sessions"));
        assert!(dir.to_str().unwrap().contains("test-session"));
    }

    #[tokio::test]
    async fn test_get_session_memory_content_for_compact_empty() {
        let template = "# Session Title\n_A title._\n";
        let content = get_session_memory_content_for_compact(
            Path::new("/nonexistent/memory.md"),
            template,
            12000,
        )
        .await
        .unwrap();

        assert_eq!(content, template);
    }

    #[test]
    fn test_recovery_context_generate_message_template() {
        let ctx = SessionMemoryRecoveryContext {
            memory_path: PathBuf::from("/workspace/sessions/test/memory.md"),
            content: "# Session Title\n_A title._\n".to_string(),
            is_template: true,
            extraction_started_at: None,
        };

        let msg = ctx.generate_recovery_message();
        assert!(msg.contains("no content yet"));
    }

    #[test]
    fn test_recovery_context_generate_message_with_content() {
        let ctx = SessionMemoryRecoveryContext {
            memory_path: PathBuf::from("/workspace/sessions/test/memory.md"),
            content: "# Session Title\nMy Session\n\n# Current State\nWorking\n".to_string(),
            is_template: false,
            extraction_started_at: None,
        };

        let msg = ctx.generate_recovery_message();
        assert!(msg.contains("Session Memory Recovery"));
        assert!(msg.contains("My Session"));
    }
}
