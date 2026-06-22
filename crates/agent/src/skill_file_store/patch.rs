//! SKILL.md 内容打补丁的模糊匹配逻辑。
//!
//! `patch_skill_content` 及其模糊匹配/相似度/分词等内部辅助从
//! `skill_file_store.rs` 抽出，供 patch action 调用。

use blockcell_core::{Error, Result};

pub(super) fn patch_skill_content(
    current: &str,
    old_text: &str,
    replacement: &str,
) -> Result<String> {
    let count = current.matches(old_text).count();
    if count == 1 {
        return Ok(current.replacen(old_text, replacement, 1));
    }
    if count > 1 {
        return Err(patch_match_error(
            "old_text is ambiguous",
            old_text,
            current,
            current
                .match_indices(old_text)
                .map(|(start, text)| (start, start + text.len()))
                .collect(),
        ));
    }

    let candidates = fuzzy_patch_candidates(current, old_text);
    let (start, end) = match candidates.as_slice() {
        [only] => (only.start, only.end),
        [best, second, ..] if best.score - second.score >= 0.20 && best.score >= 0.92 => {
            (best.start, best.end)
        }
        [] => {
            return Err(patch_match_error(
                "old_text did not match any unique location",
                old_text,
                current,
                Vec::new(),
            ));
        }
        _ => {
            return Err(patch_match_error(
                "old_text fuzzy match is ambiguous",
                old_text,
                current,
                candidates
                    .iter()
                    .map(|candidate| (candidate.start, candidate.end))
                    .collect(),
            ));
        }
    };
    let mut next = String::with_capacity(current.len() - (end - start) + replacement.len());
    next.push_str(&current[..start]);
    next.push_str(replacement);
    next.push_str(&current[end..]);
    Ok(next)
}

#[derive(Debug, Clone)]
struct PatchCandidate {
    start: usize,
    end: usize,
    score: f64,
}

fn fuzzy_patch_candidates(haystack: &str, needle: &str) -> Vec<PatchCandidate> {
    let needle_tokens = tokenize_patch_match_text(needle);
    if needle_tokens.len() < 4 {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for (start, end) in fuzzy_patch_segments(haystack) {
        let span_tokens = tokenize_patch_match_text(&haystack[start..end]);
        if span_tokens.is_empty() {
            continue;
        }
        let score = patch_token_similarity(&needle_tokens, &span_tokens);
        if score >= 0.72 {
            candidates.push(PatchCandidate { start, end, score });
        }
    }

    candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
    candidates
}

fn patch_match_error(
    reason: &str,
    old_text: &str,
    current: &str,
    candidates: Vec<(usize, usize)>,
) -> Error {
    let preview = truncate_chars(current, 700);
    let candidate_previews = candidates
        .into_iter()
        .take(5)
        .map(|(start, end)| truncate_chars(current[start..end].trim(), 180))
        .collect::<Vec<_>>();
    Error::Validation(
        serde_json::json!({
            "error": reason,
            "old_text": old_text,
            "hint": "Use skill_view, then retry with a longer unique old_text that includes surrounding context.",
            "file_preview": preview,
            "candidates": candidate_previews,
        })
        .to_string(),
    )
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let truncated = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn fuzzy_patch_segments(text: &str) -> Vec<(usize, usize)> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        if matches!(ch, '.' | '!' | '?' | '\n') {
            let end = idx + ch.len_utf8();
            if end > start {
                segments.push((start, end));
            }
            start = end;
        }
    }
    if start < text.len() {
        segments.push((start, text.len()));
    }
    segments
        .into_iter()
        .map(|(start, end)| trim_span(text, start, end))
        .filter(|(start, end)| end > start)
        .collect()
}

fn trim_span(text: &str, mut start: usize, mut end: usize) -> (usize, usize) {
    while start < end {
        let Some(ch) = text[start..end].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        start += ch.len_utf8();
    }
    while start < end {
        let Some(ch) = text[start..end].chars().next_back() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        end -= ch.len_utf8();
    }
    (start, end)
}

fn tokenize_patch_match_text(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn patch_token_similarity(left: &[String], right: &[String]) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut dp = vec![vec![0usize; right.len() + 1]; left.len() + 1];
    for i in 0..left.len() {
        for (j, right_token) in right.iter().enumerate() {
            dp[i + 1][j + 1] = if &left[i] == right_token {
                dp[i][j] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let common = dp[left.len()][right.len()] as f64;
    common / left.len().max(right.len()) as f64
}
