use sha2::{Digest, Sha256};

/// 计算 session_key 的稳定哈希后缀（64 位，16 位十六进制字符）。
///
/// 使用 SHA-256 取前 8 字节，保证跨平台、跨 Rust 版本的一致性。
/// 用于文件系统路径生成，避免 [`std::collections::hash_map::DefaultHasher`]
/// 在不同 Rust 版本间不兼容的问题。
///
/// ## 设计决策
/// - 使用 SHA-256 而非 DefaultHasher：保证输出是稳定持久化格式契约
/// - 截取 64 bit（而非 32 bit）：session 数量较大时降低碰撞风险
/// - 所有写入侧和读取侧（如 `session_recall`）必须使用同一实现
pub fn stable_hash_session_key(session_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_key.as_bytes());
    let result = hasher.finalize();
    // 取前 8 字节（64 位）作为十六进制后缀
    // SHA-256 输出固定 32 字节，直接索引构造 [u8; 8] 安全无 panic
    let hash_u64 = u64::from_be_bytes([
        result[0], result[1], result[2], result[3],
        result[4], result[5], result[6], result[7],
    ]);
    format!("{:016x}", hash_u64)
}

pub fn build_session_key(channel: &str, chat_id: &str) -> String {
    format!("{}:{}", channel, chat_id)
}

/// 将 session_key 转换为文件系统安全的文件名（stem）。
///
/// 编码规则：`:`、`/`、`\` → `_`
///
/// ## 已知限制
/// chat_id 中若含有下划线 `_`，会导致 `session_title_from_id` 将其误转为 `:`（因为该函数
/// 将所有 `_` 还原为 `:`）。在 chat_id 包含 `_` 的场景中，round-trip 会丢失信息。
/// 未来改进：使用更健壮的分隔符（如 percent-encoding 或 double-underscore `__`）。
pub fn session_file_stem(session_key: &str) -> String {
    session_key.replace([':', '/', '\\'], "_")
}

/// 从文件 stem 中提取 session_id（即 channel 之后的部分）。
///
/// 使用第一个 `_` 作为 channel 与 chat_id 的分界。若 chat_id 中包含 `_`，
/// 不会被错误截断——但后续 `session_title_from_id` 会将所有 `_` 转为 `:`，
/// 导致 chat_id 中原有的 `_` 丢失。参见 [`session_file_stem`] 的已知限制。
pub fn session_id_from_file_stem(file_stem: &str) -> String {
    file_stem
        .find('_')
        .map(|pos| file_stem[pos + 1..].to_string())
        .unwrap_or_else(|| file_stem.to_string())
}

pub fn session_title_from_id(session_id: &str) -> String {
    session_id.replace('_', ":")
}

pub fn resolve_session_key_from_id<'a, I>(session_id: &str, file_stems: I) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    let stems: Vec<&str> = file_stems.into_iter().collect();
    let normalized_id = session_id.replace(':', "_");
    let direct_key = build_session_key("ws", &session_title_from_id(session_id));
    let direct_stem = session_file_stem(&direct_key);

    if stems.iter().any(|stem| **stem == direct_stem) {
        return direct_key;
    }

    for file_stem in stems {
        if file_stem == normalized_id || session_id_from_file_stem(file_stem) == normalized_id {
            return file_stem.replace('_', ":");
        }
    }

    direct_key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_session_key() {
        assert_eq!(build_session_key("ws", "default:123"), "ws:default:123");
    }

    #[test]
    fn test_session_file_stem() {
        assert_eq!(session_file_stem("ws:default:123"), "ws_default_123");
        assert_eq!(session_file_stem("cli/run\\test"), "cli_run_test");
    }

    #[test]
    fn test_session_id_from_file_stem() {
        assert_eq!(session_id_from_file_stem("ws_default_123"), "default_123");
        assert_eq!(session_id_from_file_stem("default_123"), "123");
    }

    #[test]
    fn test_resolve_session_key_from_id_prefers_existing_direct_stem() {
        let stems = ["ws_default_123", "telegram_chat_1"];
        assert_eq!(
            resolve_session_key_from_id("default_123", stems.iter().copied()),
            "ws:default:123"
        );
    }

    #[test]
    fn test_resolve_session_key_from_id_falls_back_to_matching_stem() {
        let stems = ["ws_ws_default_123", "telegram_chat_1"];
        assert_eq!(
            resolve_session_key_from_id("ws_default_123", stems.iter().copied()),
            "ws:ws:default:123"
        );
    }
}
