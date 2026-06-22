//! Token estimation utilities for context management.
//!
//! Provides accurate token counting for chat messages using tiktoken-rs.
//! Falls back to conservative estimation if tiktoken fails to initialize.
//!
//! Supported models:
//! - GPT-3.5-turbo, GPT-4 (cl100k_base encoding)
//! - DeepSeek, Claude, and other OpenAI-compatible models

use blockcell_core::types::ChatMessage;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;

/// Global tiktoken encoder using cl100k_base (GPT-3.5-turbo, GPT-4).
///
/// This is initialized once and reused across all token counting operations.
/// If initialization fails (e.g., network issues downloading vocabulary),
/// we fall back to conservative estimation.
static TIKTOKEN_ENCODER: LazyLock<Option<Arc<tiktoken_rs::CoreBPE>>> = LazyLock::new(|| {
    match tiktoken_rs::cl100k_base() {
        Ok(encoder) => {
            tracing::debug!("[token] tiktoken encoder initialized successfully");
            Some(Arc::new(encoder))
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "[token] Failed to initialize tiktoken encoder, falling back to conservative estimation"
            );
            None
        }
    }
});

/// Count tokens using tiktoken if available, otherwise fall back to conservative estimation.
///
/// This is the primary token counting function for text content.
pub(crate) fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    // Try tiktoken first
    if let Some(encoder) = TIKTOKEN_ENCODER.as_ref() {
        return encoder.encode_with_special_tokens(text).len();
    }

    // Fallback: conservative estimation
    estimate_tokens_fallback(text)
}

/// Conservative token estimation fallback.
///
/// Used when tiktoken is not available. This is intentionally conservative
/// (over-estimates) to avoid context overflow.
///
/// - Chinese characters ≈ 1 token each
/// - English words ≈ 1.3 tokens each
fn estimate_tokens_fallback(text: &str) -> usize {
    let mut tokens: usize = 0;
    let mut ascii_word_chars: usize = 0;

    for ch in text.chars() {
        if ch.is_ascii() {
            if ch.is_ascii_whitespace() || ch.is_ascii_punctuation() {
                if ascii_word_chars > 0 {
                    // ~1.3 tokens per English word, round up
                    tokens += 1 + ascii_word_chars / blockcell_core::CHARS_PER_TOKEN;
                    ascii_word_chars = 0;
                }
                // whitespace/punctuation: ~0.25 tokens each, batch them
                tokens += 1;
            } else {
                ascii_word_chars += 1;
            }
        } else {
            // Flush pending ASCII word
            if ascii_word_chars > 0 {
                tokens += 1 + ascii_word_chars / blockcell_core::CHARS_PER_TOKEN;
                ascii_word_chars = 0;
            }
            // CJK and other multi-byte: ~1 token per character
            tokens += 1;
        }
    }

    // Flush trailing ASCII word
    if ascii_word_chars > 0 {
        tokens += 1 + ascii_word_chars / blockcell_core::CHARS_PER_TOKEN;
    }

    // Add per-message overhead (role markers, formatting)
    tokens + 4
}

/// Estimate tokens for reasoning content in a ChatMessage.
pub(crate) fn estimate_thinking_tokens(msg: &ChatMessage) -> usize {
    msg.reasoning_content
        .as_ref()
        .map(|r| estimate_tokens(r))
        .unwrap_or(0)
}

/// Process-wide, bounded memoization cache for per-message token counts.
///
/// Keyed by a 64-bit fingerprint of the message's token-relevant fields
/// (role + content + reasoning + tool calls). Chat messages are effectively
/// immutable once created, so a content fingerprint stays correct even if a
/// message is later mutated in place — a changed message simply produces a new
/// fingerprint and is recomputed. This turns the repeated full-history tiktoken
/// re-encoding done on every agent loop iteration into cheap fingerprint hashing
/// plus a single encode per distinct message.
static TOKEN_MEMO: LazyLock<Mutex<HashMap<u64, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Upper bound on cached entries. When exceeded the cache is cleared wholesale,
/// keeping memory bounded for long-running processes (e.g. the gateway).
const TOKEN_MEMO_CAP: usize = 16_384;

/// Compute a cheap fingerprint of the token-relevant fields of a message.
fn message_fingerprint(msg: &ChatMessage) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    msg.role.hash(&mut hasher);
    match &msg.content {
        serde_json::Value::String(s) => {
            0u8.hash(&mut hasher);
            s.hash(&mut hasher);
        }
        other => {
            1u8.hash(&mut hasher);
            other.to_string().hash(&mut hasher);
        }
    }
    if let Some(reasoning) = msg.reasoning_content.as_ref() {
        reasoning.hash(&mut hasher);
    }
    if let Some(calls) = msg.tool_calls.as_ref() {
        for call in calls {
            call.name.hash(&mut hasher);
            call.arguments.to_string().hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Uncached per-message token estimation (content + tool_calls + thinking overhead).
fn estimate_message_tokens_uncached(msg: &ChatMessage) -> usize {
    let content_tokens = match &msg.content {
        serde_json::Value::String(s) => estimate_tokens(s),
        serde_json::Value::Array(parts) => {
            parts
                .iter()
                .map(|p| {
                    if let Some(text) = p.get("text").and_then(|t| t.as_str()) {
                        estimate_tokens(text)
                    } else if p.get("image_url").is_some() {
                        // Base64 images: ~85 tokens for low-detail, ~765 for high-detail
                        // Use conservative estimate
                        200
                    } else {
                        10
                    }
                })
                .sum()
        }
        _ => 0,
    };

    let tool_call_tokens = msg.tool_calls.as_ref().map_or(0, |calls| {
        calls
            .iter()
            .map(|tc| estimate_tokens(&tc.name) + estimate_tokens(&tc.arguments.to_string()) + 10)
            .sum()
    });

    let thinking_tokens = estimate_thinking_tokens(msg);

    content_tokens + tool_call_tokens + thinking_tokens + 4 // role overhead
}

/// Estimate the total token count for a slice of chat messages.
///
/// Acquires the memoization cache once for the whole slice (rather than once per
/// message) so the common all-cached path costs a single lock plus per-message
/// fingerprint hashing — avoiding the previous O(N) tiktoken re-encode on every
/// agent loop iteration.
pub(crate) fn estimate_messages_tokens(messages: &[ChatMessage]) -> usize {
    let mut cache = TOKEN_MEMO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.len() >= TOKEN_MEMO_CAP {
        cache.clear();
    }
    let mut total = 0;
    for msg in messages {
        let fingerprint = message_fingerprint(msg);
        let tokens = match cache.get(&fingerprint) {
            Some(&tokens) => tokens,
            None => {
                let tokens = estimate_message_tokens_uncached(msg);
                cache.insert(fingerprint, tokens);
                tokens
            }
        };
        total += tokens;
    }
    total
}

/// Check if tiktoken encoder is available.
///
/// Returns true if the tiktoken encoder was successfully initialized.
#[allow(dead_code)]
pub fn is_tiktoken_available() -> bool {
    TIKTOKEN_ENCODER.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_ascii() {
        let tokens = estimate_tokens("hello world");
        // "hello world" should be 2-3 tokens with tiktoken
        assert!(tokens > 0 && tokens < 10, "Got {} tokens", tokens);
    }

    #[test]
    fn test_estimate_tokens_cjk() {
        let tokens = estimate_tokens("你好世界");
        // "你好世界" should be 4 tokens with tiktoken (each char is a token)
        assert!((4..=10).contains(&tokens), "Got {} tokens", tokens);
    }

    #[test]
    fn test_estimate_tokens_mixed() {
        let tokens = estimate_tokens("hello 你好 world 世界");
        assert!(tokens > 5 && tokens < 20, "Got {} tokens", tokens);
    }

    #[test]
    fn test_estimate_message_tokens_simple() {
        let msg = ChatMessage::user("测试消息");
        let tokens = estimate_messages_tokens(std::slice::from_ref(&msg));
        assert!(tokens > 0);
    }

    #[test]
    fn test_memoized_matches_uncached() {
        // Cached batch estimate must equal the sum of uncached per-message estimates.
        let messages = vec![
            ChatMessage::user("hello 你好 world"),
            ChatMessage::assistant("a longer assistant reply with some content"),
            ChatMessage::tool_result("call_1", "{\"result\": [1, 2, 3]}"),
        ];
        let uncached: usize = messages.iter().map(estimate_message_tokens_uncached).sum();
        let cached = estimate_messages_tokens(&messages);
        // Run twice to exercise the cache-hit path.
        let cached_again = estimate_messages_tokens(&messages);
        assert_eq!(cached, uncached);
        assert_eq!(cached_again, uncached);
    }

    #[test]
    fn test_estimate_messages_tokens_multiple() {
        let messages = vec![ChatMessage::user("hello"), ChatMessage::assistant("world")];
        let tokens = estimate_messages_tokens(&messages);
        assert!(tokens > 0);
    }

    #[test]
    fn test_fallback_estimation() {
        // Test that fallback works
        let tokens = estimate_tokens_fallback("hello world");
        assert!(tokens > 0);

        let tokens = estimate_tokens_fallback("你好世界");
        assert!(tokens > 0);
    }

    #[test]
    fn test_tiktoken_accuracy() {
        // If tiktoken is available, test accuracy
        if let Some(encoder) = TIKTOKEN_ENCODER.as_ref() {
            // Test some known cases
            let text = "The quick brown fox jumps over the lazy dog.";
            let tokens = encoder.encode_with_special_tokens(text);
            println!("'{}' -> {} tokens", text, tokens.len());

            let text_cn = "人工智能正在改变世界";
            let tokens_cn = encoder.encode_with_special_tokens(text_cn);
            println!("'{}' -> {} tokens", text_cn, tokens_cn.len());
        }
    }
}
