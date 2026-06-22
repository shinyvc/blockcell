//! Ghost（自进化/学习）相关配置类型。
//!
//! 包含 GhostLearningConfig、GhostConfig。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GhostLearningConfig {
    #[serde(default = "default_ghost_learning_enabled")]
    pub enabled: bool,
    #[serde(default = "default_ghost_learning_shadow_mode")]
    pub shadow_mode: bool,
    #[serde(default = "default_ghost_turn_review_interval")]
    pub turn_review_interval: u32,
    #[serde(default = "default_ghost_method_tool_threshold")]
    pub method_tool_threshold: u32,
    #[serde(default = "default_ghost_recall_max_items")]
    pub recall_max_items: u32,
    #[serde(default = "default_ghost_recall_token_budget")]
    pub recall_token_budget: u32,
}

fn default_ghost_learning_enabled() -> bool {
    true
}

fn default_ghost_learning_shadow_mode() -> bool {
    true
}

fn default_ghost_turn_review_interval() -> u32 {
    6
}

fn default_ghost_method_tool_threshold() -> u32 {
    3
}

fn default_ghost_recall_max_items() -> u32 {
    4
}

fn default_ghost_recall_token_budget() -> u32 {
    1200
}

impl Default for GhostLearningConfig {
    fn default() -> Self {
        Self {
            enabled: default_ghost_learning_enabled(),
            shadow_mode: default_ghost_learning_shadow_mode(),
            turn_review_interval: default_ghost_turn_review_interval(),
            method_tool_threshold: default_ghost_method_tool_threshold(),
            recall_max_items: default_ghost_recall_max_items(),
            recall_token_budget: default_ghost_recall_token_budget(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GhostConfig {
    #[serde(default = "default_ghost_enabled")]
    pub enabled: bool,
    /// If None, uses the default agent model.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_ghost_schedule")]
    pub schedule: String,
    #[serde(default = "default_max_syncs")]
    pub max_syncs_per_day: u32,
    #[serde(default = "default_auto_social")]
    pub auto_social: bool,
    #[serde(default)]
    pub learning: GhostLearningConfig,
}

fn default_ghost_enabled() -> bool {
    false
}

fn default_ghost_schedule() -> String {
    "0 */4 * * *".to_string() // Every 4 hours
}

fn default_max_syncs() -> u32 {
    10
}

fn default_auto_social() -> bool {
    true
}

impl Default for GhostConfig {
    fn default() -> Self {
        Self {
            enabled: default_ghost_enabled(),
            model: None,
            schedule: default_ghost_schedule(),
            max_syncs_per_day: default_max_syncs(),
            auto_social: default_auto_social(),
            learning: GhostLearningConfig::default(),
        }
    }
}
