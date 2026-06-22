//! Markdown 解析与摘要工具函数。
//!
//! 从 SKILL.md 解析章节/锚点/链接、解析本地 markdown 引用路径，
//! 以及生成简洁摘要。供 `SkillDocCache` 与 `SkillManager` 复用。

use std::path::{Path, PathBuf};

use blockcell_core::Result;

#[derive(Debug, Clone)]
pub(super) struct MarkdownSection {
    pub(super) level: usize,
    pub(super) explicit_anchor: Option<String>,
    pub(super) slug_anchor: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Debug, Clone)]
pub(super) struct MarkdownLink {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) target: String,
}

pub(super) fn join_markdown_parts(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn parse_markdown_sections(content: &str) -> Vec<MarkdownSection> {
    let mut sections = Vec::new();
    let mut offset = 0usize;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if let Some((level, title, explicit_anchor)) = parse_heading_line(trimmed) {
            sections.push(MarkdownSection {
                level,
                explicit_anchor,
                slug_anchor: slugify_anchor(&title),
                start: offset,
                end: content.len(),
            });
        }
        offset += line.len();
    }

    for index in 0..sections.len() {
        let level = sections[index].level;
        let next_start = sections[index + 1..]
            .iter()
            .find(|candidate| candidate.level <= level)
            .map(|candidate| candidate.start)
            .unwrap_or(content.len());
        sections[index].end = next_start;
    }

    sections
}

fn parse_heading_line(line: &str) -> Option<(usize, String, Option<String>)> {
    let level = line.chars().take_while(|ch| *ch == '#').count();
    if level == 0 || level > 6 {
        return None;
    }

    let remainder = line[level..].strip_prefix(' ')?;
    let mut title = remainder.trim().to_string();
    let mut explicit_anchor = None;

    if title.ends_with('}') {
        if let Some(start) = title.rfind("{#") {
            let anchor = title[start + 2..title.len() - 1].trim();
            if !anchor.is_empty() {
                explicit_anchor = Some(anchor.to_string());
                title = title[..start].trim().to_string();
            }
        }
    }

    Some((level, title, explicit_anchor))
}

fn slugify_anchor(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_dash = false;

    for ch in value.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_alphanumeric() {
            slug.push(ch);
            previous_was_dash = false;
        } else if !previous_was_dash {
            slug.push('-');
            previous_was_dash = true;
        }
    }

    slug.trim_matches('-').to_string()
}

pub(super) fn find_section<'a>(
    sections: &'a [MarkdownSection],
    anchor: &str,
) -> Option<&'a MarkdownSection> {
    sections
        .iter()
        .find(|section| section.explicit_anchor.as_deref() == Some(anchor))
        .or_else(|| {
            let slug = slugify_anchor(anchor);
            sections
                .iter()
                .find(|section| !slug.is_empty() && section.slug_anchor == slug)
        })
}

pub(super) fn extract_section_by_anchor(content: &str, anchor: &str) -> Option<String> {
    let sections = parse_markdown_sections(content);
    let section = find_section(&sections, anchor)?;
    Some(content[section.start..section.end].trim().to_string())
}

pub(super) fn extract_markdown_links(line: &str) -> Vec<MarkdownLink> {
    let mut links = Vec::new();
    let mut search_from = 0usize;

    while let Some(label_start_rel) = line[search_from..].find('[') {
        let label_start = search_from + label_start_rel;
        let Some(label_end_rel) = line[label_start + 1..].find("](") else {
            break;
        };
        let label_end = label_start + 1 + label_end_rel;
        let Some(target_end_rel) = line[label_end + 2..].find(')') else {
            break;
        };
        let target_end = label_end + 2 + target_end_rel;
        let target = &line[label_end + 2..target_end];
        links.push(MarkdownLink {
            start: label_start,
            end: target_end + 1,
            target: target.to_string(),
        });
        search_from = target_end + 1;
    }

    links
}

fn strip_markdown_list_prefix(line: &str) -> &str {
    let trimmed = line.trim();
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim();
        }
    }

    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count > 0 {
        let rest = &trimmed[digit_count..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim();
        }
    }

    trimmed
}

pub(super) fn is_link_only_line(line: &str, link: &MarkdownLink) -> bool {
    strip_markdown_list_prefix(line) == line[link.start..link.end].trim()
}

pub(super) fn is_local_markdown_target(target: &str) -> bool {
    split_markdown_target(target).is_some()
}

pub(super) fn split_markdown_target(target: &str) -> Option<(&str, Option<&str>)> {
    if target.trim().is_empty()
        || target.starts_with('#')
        || target.contains("://")
        || target.starts_with("mailto:")
    {
        return None;
    }

    let mut parts = target.splitn(2, '#');
    let relative_path = parts.next()?.trim();
    if relative_path.is_empty() || !relative_path.ends_with(".md") {
        return None;
    }

    let anchor = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some((relative_path, anchor))
}

pub(super) fn resolve_markdown_path(skill_dir: &Path, relative_path: &str) -> Result<PathBuf> {
    let candidate = Path::new(relative_path);
    if candidate.is_absolute() {
        return Err(blockcell_core::Error::Skill(format!(
            "Skill markdown link '{}' must be relative",
            relative_path
        )));
    }

    let joined = skill_dir.join(candidate);
    let canonical_skill_dir = std::fs::canonicalize(skill_dir)?;
    let canonical_target = std::fs::canonicalize(&joined).map_err(|_| {
        blockcell_core::Error::Skill(format!(
            "Skill markdown link '{}' does not exist",
            relative_path
        ))
    })?;

    if !canonical_target.starts_with(&canonical_skill_dir) {
        return Err(blockcell_core::Error::Skill(format!(
            "Skill markdown link '{}' resolves outside the skill directory",
            relative_path
        )));
    }

    Ok(canonical_target)
}

fn strip_inline_markdown(line: &str) -> String {
    let mut output = String::new();
    let mut index = 0usize;

    while index < line.len() {
        let rest = &line[index..];
        let Some(label_start_rel) = rest.find('[') else {
            output.push_str(rest);
            break;
        };
        let label_start = index + label_start_rel;
        output.push_str(&line[index..label_start]);

        let Some(label_end_rel) = line[label_start + 1..].find("](") else {
            output.push_str(&line[label_start..]);
            break;
        };
        let label_end = label_start + 1 + label_end_rel;
        let Some(target_end_rel) = line[label_end + 2..].find(')') else {
            output.push_str(&line[label_start..]);
            break;
        };
        let target_end = label_end + 2 + target_end_rel;
        output.push_str(&line[label_start + 1..label_end]);
        index = target_end + 1;
    }

    output
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => format!("{}...", text[..idx].trim_end()),
        None => text.to_string(),
    }
}

pub(super) fn concise_markdown_excerpt(text: &str, max_chars: usize) -> String {
    let mut excerpt = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !excerpt.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }

        let cleaned = strip_inline_markdown(strip_markdown_list_prefix(trimmed))
            .replace('`', "")
            .trim()
            .to_string();
        if cleaned.is_empty() {
            continue;
        }

        if !excerpt.is_empty() {
            excerpt.push(' ');
        }
        excerpt.push_str(&cleaned);

        if excerpt.chars().count() >= max_chars {
            break;
        }
    }

    truncate_chars(excerpt.trim(), max_chars)
}
