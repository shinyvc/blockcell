//! Agent CLI 启动期的辅助逻辑：Provider/Pool 构建、技能进化 Provider 装配、
//! 媒体路径解析、以及把命令行参数解析为具体 agent 运行上下文。
//!
//! 这些都是 `run()` 启动阶段调用的独立纯函数，从 `commands/agent.rs` 抽离以
//! 缩小主文件体积，不改变任何行为。

use blockcell_agent::SkillEvolutionLLMBridge;
use blockcell_core::{Config, Paths};
use blockcell_providers::{Provider, ProviderPool};
use std::sync::Arc;
use tracing::{info, warn};

pub(super) fn create_skill_evolution_llm_provider(
    config: &Config,
    provider_pool: &ProviderPool,
) -> Option<Arc<dyn blockcell_skills::LLMProvider>> {
    let provider: Option<Arc<dyn Provider>> = if config.agents.defaults.evolution_model.is_some()
        || config.agents.defaults.evolution_provider.is_some()
    {
        match crate::commands::provider::create_evolution_provider(config) {
            Ok(evo_provider) => {
                info!("Skill evolution provider configured with independent model");
                Some(Arc::from(evo_provider))
            }
            Err(e) => {
                warn!(
                    "Failed to create skill evolution provider: {}, using main provider",
                    e
                );
                provider_pool.acquire().map(|(_, p)| p)
            }
        }
    } else {
        provider_pool.acquire().map(|(_, p)| p)
    };

    provider.map(|p| {
        Arc::new(SkillEvolutionLLMBridge::new_arc(p)) as Arc<dyn blockcell_skills::LLMProvider>
    })
}

/// Extract image file paths from user input.
/// Supports:
/// - Inline absolute paths: `/path/to/image.png what is this image`
/// - @-prefixed paths: `@/path/to/image.png recognize this`
/// - ~ home dir paths: `~/Desktop/photo.jpg take a look`
///
/// Returns (cleaned_text, media_paths).
pub(super) fn extract_media_from_input(input: &str) -> (String, Vec<String>) {
    let image_extensions = ["jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "heic"];
    let mut media = Vec::new();
    let mut text_parts = Vec::new();

    for token in input.split_whitespace() {
        let path_str = token.strip_prefix('@').unwrap_or(token);
        // Expand ~ to home dir
        let expanded: String = if let Some(rest) = path_str.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                home.join(rest).to_string_lossy().into_owned()
            } else {
                path_str.to_string()
            }
        } else {
            path_str.to_string()
        };

        let path = std::path::Path::new(&expanded);
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| image_extensions.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false);

        if is_image && path.exists() {
            media.push(expanded);
        } else {
            text_parts.push(token.to_string());
        }
    }

    let text = text_parts.join(" ");
    (text, media)
}

#[allow(dead_code)]
pub(super) fn create_provider(config: &Config) -> anyhow::Result<Box<dyn Provider>> {
    crate::commands::provider::create_provider(config)
}

pub(super) fn build_pool_with_overrides(
    config: &mut Config,
    model_override: Option<String>,
    provider_override: Option<String>,
) -> anyhow::Result<std::sync::Arc<ProviderPool>> {
    if let Some(ref m) = model_override {
        // If model_pool is already configured, clear it and use the override as a single entry
        if !config.agents.defaults.model_pool.is_empty() {
            config.agents.defaults.model_pool.clear();
        }
        config.agents.defaults.model = m.clone();
    }
    if let Some(ref p) = provider_override {
        config.agents.defaults.provider = Some(p.clone());
    }
    ProviderPool::from_config(config)
}

#[derive(Debug)]
pub(super) struct AgentCliContext {
    pub(super) agent_id: String,
    pub(super) session: String,
    pub(super) config: Config,
    pub(super) paths: Paths,
}

pub(super) fn resolve_agent_context(
    config: &Config,
    paths: &Paths,
    requested_agent: Option<&str>,
    requested_session: Option<&str>,
) -> anyhow::Result<AgentCliContext> {
    let agent_id = requested_agent
        .map(str::trim)
        .filter(|agent_id| !agent_id.is_empty())
        .unwrap_or("default");

    if !config.agent_exists(agent_id) {
        anyhow::bail!("Unknown agent '{}'", agent_id);
    }

    let agent_config = config
        .config_for_agent(agent_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown agent '{}'", agent_id))?;
    let agent_paths = paths.for_agent(agent_id);
    let session = requested_session
        .map(str::trim)
        .filter(|session| !session.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("cli:{}", agent_id));

    Ok(AgentCliContext {
        agent_id: agent_id.to_string(),
        session,
        config: agent_config,
        paths: agent_paths,
    })
}
