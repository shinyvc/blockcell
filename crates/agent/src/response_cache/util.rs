//! ResponseCache 的纯工具函数：预览截断、大小格式化、键名清洗。
//!
//! 这些都是无状态的叶子函数，从 `response_cache.rs` 抽出以缩小主文件。
//! 通过父模块 `pub use` 重导出，保持对外路径 `response_cache::*` 不变。

/// 在换行边界截断以保持可读性，确保按字符数（而非字节数）截断
pub fn generate_preview(content: &str, max_chars: usize) -> (String, bool) {
    // 按字符数判断是否需要截断
    let char_count = content.chars().count();
    if char_count <= max_chars {
        return (content.to_string(), false);
    }

    // 在字符边界处截断，查找合适的新行断点
    let char_boundary: usize = content.chars().take(max_chars).map(|c| c.len_utf8()).sum();

    // 在截断范围内查找最后一个换行符，避免在行中间截断
    let truncated = &content[..char_boundary];
    let last_newline = truncated.rfind('\n');

    // 如果找到换行符且位置合理（> 50% 限制），使用它
    let cut_point = last_newline
        .filter(|&pos| pos > char_boundary / 2)
        .unwrap_or(char_boundary);

    (content[..cut_point].to_string(), true)
}

/// 格式化字符数（用于 preview 大小显示）
pub(crate) fn format_chars(size: usize) -> String {
    if size < 1024 {
        format!("{} chars", size)
    } else if size < 1024 * 1024 {
        format!("{:.1}K chars", size as f64 / 1024.0)
    } else {
        format!("{:.1}M chars", size as f64 / (1024.0 * 1024.0))
    }
}

/// 格式化字节数（用于原始内容大小显示）
pub(crate) fn format_bytes(size: usize) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    }
}

/// 清理 session_key 用于文件系统路径，并追加哈希后缀保证不同会话的唯一性。
///
/// ## 问题背景
/// `sanitize_tool_use_id` 对 session_key 过于激进：只保留 ascii 字母数字、`-`、`_`，
/// 且截断到 64 字符。不同 session_key（如 `"wechat:user@domain"` 与 `"wechat:user#domain"`）
/// 可能映射到同一目录名，导致 session_recall 跨会话读到错误工具输出。
///
/// ## 解决方案
/// 1. 保留可读前缀（48 字符，留出空间给哈希后缀）
/// 2. 对原始 session_key 计算 8 位十六进制哈希，保证唯一性
/// 3. 即使两个 session_key 清洗后前缀相同，哈希后缀也不同
///
/// ## 使用位置
/// - 写入侧：`runtime::try_persist_large_tool_result`、`response_cache::persist_tool_result`
/// - 读取侧：`tools::session_recall`（需保持相同算法）
pub fn sanitize_session_key(session_key: &str) -> String {
    // 保留可读前缀：仅 ascii 字母数字、-、_，限制 48 字符
    let clean: String = session_key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(48)
        .collect();

    // 使用 SHA-256 稳定哈希后缀保证唯一性和跨版本兼容性
    // （与 tools/session_recall 复用同一实现，避免两边算法漂移）
    let hash_suffix = blockcell_core::stable_hash_session_key(session_key);

    if clean.is_empty() {
        format!("session_{hash_suffix}")
    } else {
        format!("{clean}_{hash_suffix}")
    }
}

/// 清理 tool_use_id 以防止路径注入
///
/// `tool_use_id` 来自 LLM 输出，可能包含：
/// - 路径遍历字符 (`../`, `..\\`)
/// - 换行符 (`\n`, `\r`)
/// - 空字符 (`\0`)
/// - 其他可能导致路径问题的字符
///
/// 清理策略：
/// 1. 只保留字母、数字、连字符和下划线
/// 2. 检查是否为 Windows 保留文件名
/// 3. 限制长度
pub fn sanitize_tool_use_id(tool_use_id: &str) -> String {
    // 移除或替换危险字符
    let sanitized: String = tool_use_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();

    // 如果清理后为空，使用默认值
    if sanitized.is_empty() {
        return format!("tool-{}", uuid::Uuid::new_v4().simple());
    }

    // 限制长度（按字符边界安全截断，避免 panic）
    let result = if sanitized.len() > 64 {
        let boundary = sanitized.floor_char_boundary(64);
        sanitized[..boundary].to_string()
    } else {
        sanitized
    };

    // 检查 Windows 保留文件名
    // CON, PRN, AUX, NUL, COM1-COM9, LPT1-LPT9
    let upper = result.to_uppercase();
    let is_reserved = matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );

    if is_reserved {
        // 添加后缀避免保留名
        format!("{}-{}", result, uuid::Uuid::new_v4().simple())
    } else {
        result
    }
}
