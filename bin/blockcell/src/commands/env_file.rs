use anyhow::Context;
use blockcell_core::Paths;
use std::fs;
use std::path::Path;
use tracing::info;

fn default_env_template() -> &'static str {
    "BLOCKCELL_API_TOKEN=\nOPENAI_API_KEY=\nANTHROPIC_API_KEY=\nGEMINI_API_KEY=\n"
}

fn parse_env_assignment(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let body = trimmed.strip_prefix("export ").unwrap_or(trimmed).trim();
    let (key, value) = body.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }

    let mut value = value.trim().to_string();
    if value.len() >= 2 {
        let quoted = (value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\''));
        if quoted {
            value = value[1..value.len() - 1].to_string();
        }
    }

    Some((key.to_string(), value))
}

pub fn ensure_and_load_blockcell_env(paths: &Paths) -> anyhow::Result<()> {
    paths.ensure_dirs().with_context(|| {
        format!(
            "failed to create blockcell dirs at {}",
            paths.base.display()
        )
    })?;

    let env_path = paths.env_file();
    if !env_path.exists() {
        fs::write(&env_path, default_env_template()).with_context(|| {
            format!(
                "failed to create default env file at {}",
                env_path.display()
            )
        })?;
        info!(path = %env_path.display(), "Created default blockcell .env file");
    }

    load_env_file(&env_path)
}

fn load_env_file(path: &Path) -> anyhow::Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read env file {}", path.display()))?;

    let mut loaded = 0usize;
    for line in content.lines() {
        if let Some((key, value)) = parse_env_assignment(line) {
            // This runs during command startup, before worker threads are spawned.
            unsafe {
                std::env::set_var(key, value);
            }
            loaded += 1;
        }
    }

    info!(path = %path.display(), loaded, "Loaded blockcell .env file");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_env_assignment;

    #[test]
    fn parse_env_assignment_handles_plain_export_and_quotes() {
        assert_eq!(
            parse_env_assignment("GBRAIN_EMBEDDING_MODEL=embedding-3"),
            Some((
                "GBRAIN_EMBEDDING_MODEL".to_string(),
                "embedding-3".to_string()
            ))
        );
        assert_eq!(
            parse_env_assignment("export GBRAIN_EMBEDDING_DIMENSIONS='2048'"),
            Some((
                "GBRAIN_EMBEDDING_DIMENSIONS".to_string(),
                "2048".to_string()
            ))
        );
    }

    #[test]
    fn parse_env_assignment_ignores_comments_and_blank_lines() {
        assert_eq!(parse_env_assignment("# GBRAIN_EMBEDDING_MODEL=x"), None);
        assert_eq!(parse_env_assignment("   "), None);
    }
}
