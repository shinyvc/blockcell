use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use crate::error::Result;
use crate::paths::Paths;

// 子模块 — 按职责拆分的配置类型
pub mod channels;
pub use channels::*;
pub mod intent;
pub use intent::*;
pub mod memory;
pub use memory::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_base: Option<String>,
    /// 该 provider 专用代理（可选）。优先级高于全局 network.proxy。
    /// 设置为空字符串 "" 可强制该 provider 直连（跳过全局代理）。
    /// 格式："http://host:port" 或 "socks5://host:port"
    #[serde(default)]
    pub proxy: Option<String>,
    /// API 接口类型："openai" | "openai_responses" | "anthropic" | "gemini" | "ollama"
    /// 用于前端显示和接口兼容性标识，默认 "openai"（序列化时省略默认值）
    #[serde(
        default = "default_api_type",
        skip_serializing_if = "is_default_api_type"
    )]
    pub api_type: String,
}

fn default_api_type() -> String {
    "openai".to_string()
}

fn is_default_api_type(t: &str) -> bool {
    t == "openai"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityHubConfig {
    #[serde(default)]
    pub hub_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    /// Short random identifier for this node (e.g. "54c6be7b").
    /// Auto-generated on first gateway startup and persisted to config.
    /// Used as the node display name in the community hub.
    #[serde(default)]
    pub node_alias: Option<String>,
}

fn default_community_hub_url() -> Option<String> {
    Some("https://hub-api.blockcell.dev".to_string())
}

impl Default for CommunityHubConfig {
    fn default() -> Self {
        Self {
            hub_url: default_community_hub_url(),
            api_key: None,
            node_alias: None,
        }
    }
}

/// 一个可用的"模型+供应商"条目，用于 model_pool 多模型高可用配置。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallMode {
    #[default]
    Native,
    Text,
    None,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    /// 模型名称，例如 "deepseek-chat-pro"、"claude-3-5-sonnet"
    pub model: String,
    /// 对应 providers 表中的 key，例如 "deepseek"、"anthropic"
    pub provider: String,
    /// 负载均衡权重（正整数，越大越优先被选中），默认 1
    #[serde(default = "default_entry_weight")]
    pub weight: u32,
    /// 优先级（小数字 = 高优先级），同优先级内按 weight 加权随机，默认 1
    #[serde(default = "default_entry_priority")]
    pub priority: u32,
    /// 输入价格（USD/1M tokens），可选
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_price: Option<f64>,
    /// 输出价格（USD/1M tokens），可选
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_price: Option<f64>,
    /// 模型专用温度参数（可选）。
    /// 若未配置，则沿用全局 `agents.defaults.temperature`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// 工具调用模式：
    /// - native: 使用 API 原生 tools/tool_calls
    /// - text: 不发送 tools，改为文本协议 <tool_call> ... </tool_call>
    /// - none: 禁用工具
    /// - auto: 先尝试 native，失败或被中继剥离后自动退化为 text
    #[serde(default, skip_serializing_if = "is_default_tool_call_mode")]
    pub tool_call_mode: ToolCallMode,
}

fn default_entry_weight() -> u32 {
    1
}
fn default_entry_priority() -> u32 {
    1
}

fn is_default_tool_call_mode(mode: &ToolCallMode) -> bool {
    matches!(mode, ToolCallMode::Native)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefaults {
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: u32,
    /// Per-tool max iterations. If a tool name is not present, use max_tool_iterations as default.
    #[serde(default)]
    pub max_tool_iterations_by_tool: HashMap<String, u32>,
    #[serde(default = "default_llm_max_retries")]
    pub llm_max_retries: u32,
    #[serde(default = "default_llm_retry_delay_ms")]
    pub llm_retry_delay_ms: u64,
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: u32,
    /// 显式指定 LLM provider（可选）
    /// 如果不指定，将从 model 字符串前缀推断（如 "anthropic/claude-..."）
    #[serde(default)]
    pub provider: Option<String>,
    /// 自进化专用模型（如果为 None，则使用主模型）
    /// 建议使用更便宜/更快的模型，避免与对话抢占并发
    #[serde(default)]
    pub evolution_model: Option<String>,
    /// 自进化专用 provider（可选）
    /// 如果不指定，将从 evolution_model 推断，或使用主 provider
    #[serde(default)]
    pub evolution_provider: Option<String>,
    /// 多模型高可用池（可选）。
    /// 配置后，系统将从池中按优先级+权重选取 provider，失败自动降级。
    /// 若留空，则沿用旧的单 model + provider 配置（向后兼容）。
    #[serde(default)]
    pub model_pool: Vec<ModelEntry>,
    /// Allowed MCP server names visible to this agent.
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    /// Allowed MCP tool names visible to this agent.
    #[serde(default)]
    pub allowed_mcp_tools: Vec<String>,
    /// 推理强度控制（DeepSeek V4 thinking mode 等）：
    /// - "off": 禁用思考 (thinking.type = disabled)
    /// - "low"/"medium"/"high": reasoning_effort = high (DeepSeek 默认)
    /// - "max": reasoning_effort = max (最深推理)
    /// - None: 不发送任何参数，由 provider 自行决定
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

fn default_workspace() -> String {
    "~/.blockcell/workspace".to_string()
}

fn default_model() -> String {
    "deepseek-v4-pro".to_string()
}

fn default_max_tokens() -> u32 {
    8192
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tool_iterations() -> u32 {
    30
}

fn default_llm_max_retries() -> u32 {
    3
}

fn default_llm_retry_delay_ms() -> u64 {
    2000
}

fn default_max_context_tokens() -> u32 {
    1_048_576 // 1M 上下文
}

fn default_true() -> bool {
    true
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            workspace: default_workspace(),
            model: default_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            max_tool_iterations: default_max_tool_iterations(),
            max_tool_iterations_by_tool: HashMap::new(),
            llm_max_retries: default_llm_max_retries(),
            llm_retry_delay_ms: default_llm_retry_delay_ms(),
            max_context_tokens: default_max_context_tokens(),
            provider: None,
            evolution_model: None,
            evolution_provider: None,
            model_pool: Vec::new(),
            allowed_mcp_servers: Vec::new(),
            allowed_mcp_tools: Vec::new(),
            reasoning_effort: None,
        }
    }
}

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

/// 全局网络代理配置。
/// 所有 LLM provider HTTP 请求默认走此代理，可被 providers.<name>.proxy 覆盖。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfig {
    /// 全局代理地址，例如 "http://127.0.0.1:7890"
    /// 留空或不配置则直连。
    #[serde(default)]
    pub proxy: Option<String>,
    /// 不走代理的域名/IP 列表，支持前缀通配符 "*.example.com"。
    /// 常见示例：["localhost", "127.0.0.1", "::1", "*.local"]
    #[serde(default)]
    pub no_proxy: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentsConfig {
    #[serde(default)]
    pub defaults: AgentDefaults,
    #[serde(default)]
    pub ghost: GhostConfig,
    /// Optional multi-agent definitions.
    /// If empty, runtime falls back to a single implicit "default" agent.
    #[serde(default)]
    pub list: Vec<AgentProfileConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileConfig {
    pub id: String,
    #[serde(default = "default_agent_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub intent_profile: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model_pool: Vec<ModelEntry>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tool_iterations: Option<u32>,
    #[serde(default)]
    pub max_tool_iterations_by_tool: HashMap<String, u32>,
    #[serde(default)]
    pub llm_max_retries: Option<u32>,
    #[serde(default)]
    pub llm_retry_delay_ms: Option<u64>,
    #[serde(default)]
    pub max_context_tokens: Option<u32>,
    #[serde(default)]
    pub evolution_model: Option<String>,
    #[serde(default)]
    pub evolution_provider: Option<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_mcp_tools: Option<Vec<String>>,
}

fn default_agent_enabled() -> bool {
    true
}

impl Default for AgentProfileConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            name: None,
            intent_profile: None,
            model: None,
            provider: None,
            model_pool: Vec::new(),
            max_tokens: None,
            temperature: None,
            max_tool_iterations: None,
            max_tool_iterations_by_tool: HashMap::new(),
            llm_max_retries: None,
            llm_retry_delay_ms: None,
            max_context_tokens: None,
            evolution_model: None,
            evolution_provider: None,
            allowed_mcp_servers: None,
            allowed_mcp_tools: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAgentConfig {
    pub id: String,
    pub name: Option<String>,
    pub defaults: AgentDefaults,
    pub intent_profile: Option<String>,
}

/// 日志配置。
/// 控制日志输出方式和等级。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogConfig {
    /// 日志等级: trace, debug, info, warn, error, off。默认: info
    #[serde(default = "default_log_level")]
    pub level: String,
    /// 是否输出到文件。默认: false
    #[serde(default)]
    pub file_enabled: bool,
    /// 是否输出到控制台。默认: true
    #[serde(default = "default_true")]
    pub console_enabled: bool,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_enabled: false,
            console_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfig {
    #[serde(default = "default_gateway_host")]
    pub host: String,
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    #[serde(default = "default_webui_host")]
    pub webui_host: String,
    #[serde(default = "default_webui_port")]
    pub webui_port: u16,
    /// Optional public API base URL injected into WebUI at runtime.
    /// Example: "https://your-domain.example.com" or "https://your-domain.example.com/api".
    /// If not set, WebUI will default to current hostname + gateway.port.
    #[serde(default)]
    pub public_api_base: Option<String>,
    #[serde(default)]
    pub api_token: Option<String>,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// WebUI login password. If empty/None, a temporary password is printed at startup.
    #[serde(default)]
    pub webui_pass: Option<String>,
}

fn default_gateway_host() -> String {
    "localhost".to_string()
}

fn default_gateway_port() -> u16 {
    18790
}

fn default_webui_host() -> String {
    "localhost".to_string()
}

fn default_webui_port() -> u16 {
    18791
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_gateway_host(),
            port: default_gateway_port(),
            webui_host: default_webui_host(),
            webui_port: default_webui_port(),
            public_api_base: None,
            api_token: None,
            allowed_origins: vec![],
            webui_pass: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchConfig {
    /// Brave Search API key (optional, for brave_search)
    #[serde(default)]
    pub api_key: String,
    /// Baidu AI Search API key (optional, for baidu_search)
    /// Get from https://qianfan.baidubce.com — set env BAIDU_API_KEY or this field
    #[serde(default)]
    pub baidu_api_key: String,
    #[serde(default = "default_max_results")]
    pub max_results: u32,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            baidu_api_key: String::new(),
            max_results: default_max_results(),
        }
    }
}

fn default_max_results() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecConfig {
    #[serde(default = "default_exec_timeout")]
    pub timeout: u32,
    #[serde(default)]
    pub restrict_to_workspace: bool,
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            timeout: default_exec_timeout(),
            restrict_to_workspace: false,
        }
    }
}

fn default_exec_timeout() -> u32 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebToolsConfig {
    #[serde(default)]
    pub search: WebSearchConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsConfig {
    #[serde(default)]
    pub web: WebToolsConfig,
    #[serde(default)]
    pub exec: ExecConfig,
    /// Tick interval in seconds for the agent runtime loop (alert checks, cron, evolution).
    /// Lower values enable faster alert response. Default: 30. Min: 10. Max: 300.
    #[serde(default = "default_tick_interval")]
    pub tick_interval_secs: u32,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            web: WebToolsConfig::default(),
            exec: ExecConfig::default(),
            tick_interval_secs: default_tick_interval(),
        }
    }
}

fn default_tick_interval() -> u32 {
    30
}

/// Configuration for the path-access policy system.
/// Points to the separate `path_access.json5` rules file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathAccessConfig {
    /// Whether the path-access policy system is active.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Path to the rules file. Supports `~/` expansion.
    #[serde(default = "default_path_access_policy_file")]
    pub policy_file: String,

    /// Behavior when the policy file is missing or unparseable.
    /// One of: `"fallback_to_safe_default"` | `"fail_closed"` | `"disabled"`
    #[serde(default = "default_missing_file_policy")]
    pub missing_file_policy: String,

    /// Reserved for future hot-reload support.
    #[serde(default)]
    pub reload_on_change: bool,
}

fn default_path_access_policy_file() -> String {
    "~/.blockcell/path_access.json5".to_string()
}

fn default_missing_file_policy() -> String {
    "fallback_to_safe_default".to_string()
}

impl Default for PathAccessConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            policy_file: default_path_access_policy_file(),
            missing_file_policy: default_missing_file_policy(),
            reload_on_change: false,
        }
    }
}

/// Top-level security settings for the agent runtime.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecurityConfig {
    /// Path-access policy rules.
    #[serde(default)]
    pub path_access: PathAccessConfig,
}

fn default_memory_vector_table() -> String {
    "memory_vectors".to_string()
}

// 内存与进化系统配置 — 已移至 config/memory.rs
// (模块声明在文件顶部)

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoUpgradeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_upgrade_channel")]
    pub channel: String,
    #[serde(default = "default_manifest_url")]
    pub manifest_url: String,
    #[serde(default = "default_require_signature")]
    pub require_signature: bool,
    #[serde(default)]
    pub maintenance_window: String,
}

fn default_upgrade_channel() -> String {
    "stable".to_string()
}

fn default_require_signature() -> bool {
    false
}

fn default_manifest_url() -> String {
    "https://github.com/blockcell-labs/blockcell/releases/latest/download/manifest.json".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub community_hub: CommunityHubConfig,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub channels: ChannelsConfig,
    /// Simplified multi-agent routing table: channel -> owner agent id.
    #[serde(default)]
    pub channel_owners: HashMap<String, String>,
    /// Account-level routing overrides: channel -> account_id -> owner agent id.
    #[serde(default)]
    pub channel_account_owners: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(
        default = "intent::default_intent_router_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub intent_router: Option<IntentRouterConfig>,
    #[serde(default)]
    pub auto_upgrade: AutoUpgradeConfig,
    /// 日志配置（等级、输出方式）
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    /// Default timezone for cron jobs and time-related operations.
    /// IANA timezone name, e.g., "Asia/Shanghai", "America/New_York", "Europe/London".
    /// If not set, system timezone is detected, falling back to UTC.
    #[serde(default)]
    pub default_timezone: Option<String>,
    /// Cron service tick interval in seconds. Default: 1 second. Min: 1. Max: 3600.
    /// Higher values reduce CPU/disk I/O but lower time precision.
    #[serde(default = "default_cron_tick_interval")]
    pub cron_tick_interval_secs: u64,
    /// 是否启用 OpenClaw skill 兼容加载（默认 false）
    #[serde(default)]
    pub openclaw_skill_enabled: bool,
    /// Self-Improve 配置 (Nudge + Review)
    #[serde(default)]
    pub self_improve: SelfImproveConfig,
    /// 进化服务配置（错误阈值、冷却期等）
    #[serde(default)]
    pub evolution: EvolutionConfig,
}

fn default_cron_tick_interval() -> u64 {
    1
}

/// Minimum allowed cron tick interval in seconds.
const MIN_CRON_TICK_INTERVAL_SECS: u64 = 1;
/// Maximum allowed cron tick interval in seconds.
const MAX_CRON_TICK_INTERVAL_SECS: u64 = 3600;

impl Default for Config {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert(
            "openrouter".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://openrouter.ai/api/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert("anthropic".to_string(), ProviderConfig::default());
        providers.insert("openai".to_string(), ProviderConfig::default());
        providers.insert("deepseek".to_string(), ProviderConfig::default());
        providers.insert(
            "groq".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://api.groq.com/openai/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert("zhipu".to_string(), ProviderConfig::default());
        providers.insert(
            "vllm".to_string(),
            ProviderConfig {
                api_key: "dummy".to_string(),
                api_base: Some("http://localhost:8000/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert(
            "gemini".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some(
                    "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
                ),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert(
            "kimi".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://api.moonshot.cn/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert(
            "xai".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://api.x.ai/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert(
            "mistral".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://api.mistral.ai/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert(
            "minimax".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://api.minimaxi.com/v1".to_string()),
                proxy: None,
                api_type: "anthropic".to_string(),
            },
        );
        providers.insert(
            "qwen".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://api.qwen.ai/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert(
            "glm".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://api.z.ai/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert(
            "siliconflow".to_string(),
            ProviderConfig {
                api_key: String::new(),
                api_base: Some("https://api.siliconflow.cn/v1".to_string()),
                proxy: None,
                api_type: "openai".to_string(),
            },
        );
        providers.insert(
            "ollama".to_string(),
            ProviderConfig {
                api_key: "ollama".to_string(),
                api_base: Some("http://localhost:11434".to_string()),
                proxy: None,
                api_type: "ollama".to_string(),
            },
        );

        Self {
            providers,
            memory: MemoryConfig::default(),
            network: NetworkConfig::default(),
            community_hub: CommunityHubConfig::default(),
            agents: AgentsConfig::default(),
            channels: ChannelsConfig::default(),
            channel_owners: HashMap::new(),
            channel_account_owners: HashMap::new(),
            gateway: GatewayConfig::default(),
            tools: ToolsConfig::default(),
            intent_router: Some(IntentRouterConfig::default()),
            auto_upgrade: AutoUpgradeConfig::default(),
            log: LogConfig::default(),
            security: SecurityConfig::default(),
            default_timezone: None,
            cron_tick_interval_secs: default_cron_tick_interval(),
            openclaw_skill_enabled: false,
            self_improve: SelfImproveConfig::default(),
            evolution: EvolutionConfig::default(),
        }
    }
}

fn format_json5_parse_error(
    path: Option<&Path>,
    context: &str,
    error: &json5::Error,
) -> crate::error::Error {
    let path_text = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<inline>".to_string());
    crate::error::Error::Config(format!(
        "{} parse error in {}: {}",
        context, path_text, error
    ))
}

fn expand_env_vars_in_text(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut index = 0usize;

    while let Some(relative_start) = content[index..].find("${") {
        let start = index + relative_start;
        out.push_str(&content[index..start]);

        let expr_start = start + 2;
        if let Some(relative_end) = content[expr_start..].find('}') {
            let end = expr_start + relative_end;
            let expr = &content[expr_start..end];
            out.push_str(&expand_env_expr(expr));
            index = end + 1;
        } else {
            out.push_str(&content[start..]);
            return out;
        }
    }

    out.push_str(&content[index..]);
    out
}

fn expand_env_expr(expr: &str) -> String {
    if let Some((name, default)) = expr.split_once(":-") {
        let name = name.trim();
        if name.is_empty() {
            return String::new();
        }
        return std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| default.to_string());
    }

    let name = expr.trim();
    if name.is_empty() {
        return String::new();
    }

    std::env::var(name).unwrap_or_default()
}

pub fn parse_json5_str<T>(content: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    parse_json5_str_with_context(content, None, "JSON5")
}

pub fn parse_json5_str_with_context<T>(
    content: &str,
    path: Option<&Path>,
    context: &str,
) -> Result<T>
where
    T: DeserializeOwned,
{
    let expanded = expand_env_vars_in_text(content);
    json5::from_str(&expanded).map_err(|e| format_json5_parse_error(path, context, &e))
}

pub fn parse_json5_value(content: &str) -> Result<Value> {
    parse_json5_str(content)
}

pub fn stringify_json5_pretty<T>(value: &T) -> Result<String>
where
    T: Serialize,
{
    Ok(serde_json::to_string_pretty(value)?)
}

pub fn write_json5_pretty<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let content = stringify_json5_pretty(value)?;
    write_text_atomic_durable(path, &content)
}

#[cfg(windows)]
fn replace_file_atomic_durable(tmp_path: &Path, path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let tmp_wide: Vec<u16> = tmp_path.as_os_str().encode_wide().chain(Some(0)).collect();
    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        MoveFileExW(
            tmp_wide.as_ptr(),
            path_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if ok == 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file_atomic_durable(tmp_path: &Path, path: &Path) -> Result<()> {
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

fn write_text_atomic_durable(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp_path = parent.join(format!(
        ".{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        nonce
    ));

    {
        let mut file = std::fs::File::create(&tmp_path)?;
        use std::io::Write;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }

    replace_file_atomic_durable(&tmp_path, path)?;

    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }

    Ok(())
}

pub fn validate_config_json5_str(content: &str) -> Result<Config> {
    parse_json5_str(content)
}

pub fn validate_config_json5_file(path: &Path, content: &str) -> Result<Config> {
    parse_json5_str_with_context(content, Some(path), "Config JSON5")
}

pub fn write_raw_validated_config_json5(path: &Path, content: &str) -> Result<Config> {
    let config = validate_config_json5_str(content)?;
    write_text_atomic_durable(path, content)?;
    Ok(config)
}

/// Detect system timezone using iana-time-zone crate.
/// Returns None if detection fails (will fall back to UTC in calling code).
fn detect_system_timezone() -> Option<String> {
    match iana_time_zone::get_timezone() {
        Ok(tz) if !tz.is_empty() => {
            // Validate the detected timezone is a valid IANA timezone
            if tz.parse::<chrono_tz::Tz>().is_ok() {
                tracing::info!(timezone = %tz, "Detected system timezone");
                Some(tz)
            } else {
                tracing::warn!(timezone = %tz, "Detected timezone is not a valid IANA timezone, falling back to UTC");
                None
            }
        }
        Ok(_) => {
            tracing::debug!("System timezone detection returned empty string, using UTC");
            None
        }
        Err(e) => {
            tracing::debug!(error = %e, "Failed to detect system timezone, using UTC");
            None
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = validate_config_json5_file(path, &content)?;
        config.validate()
    }

    /// Load config from file, or create default if not exists.
    /// Also ensures default_timezone and cron_tick_interval_secs are set,
    /// updating the config file if necessary.
    pub fn load_or_default(paths: &Paths) -> Result<Self> {
        let config_path = paths.config_file();

        let config = if config_path.exists() {
            Self::load(&config_path)?
        } else {
            // New config: detect system timezone once
            let detected_tz = detect_system_timezone();
            if let Some(ref tz) = detected_tz {
                tracing::info!(timezone = %tz, "Detected system timezone for new config");
            }
            Self {
                default_timezone: detected_tz,
                ..Default::default()
            }
        };

        // Check if we need to update the config file with missing fields
        let mut needs_save = config.default_timezone.is_none() && config_path.exists();

        // Ensure openclawSkillEnabled field exists in config file
        if config_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&config_path) {
                if !raw.contains("openclawSkillEnabled") {
                    tracing::info!("Adding missing openclawSkillEnabled field to config");
                    needs_save = true;
                }
                // Ensure loadAllTools field exists in intentRouter (if present)
                if raw.contains("intentRouter") && !raw.contains("loadAllTools") {
                    tracing::info!("Adding missing loadAllTools field to intentRouter config");
                    needs_save = true;
                }
                // Ensure log config section exists
                if !raw.contains("\"log\"") && !raw.contains("log:") {
                    tracing::info!("Adding missing log config section to config");
                    needs_save = true;
                } else {
                    // log section exists, check for missing fields
                    if !raw.contains("consoleEnabled") {
                        tracing::info!(
                            "Adding missing consoleEnabled field to log config (default: true)"
                        );
                        needs_save = true;
                    }
                    if !raw.contains("fileEnabled") {
                        tracing::info!(
                            "Adding missing fileEnabled field to log config (default: false)"
                        );
                        needs_save = true;
                    }
                    if !raw.contains("level") {
                        tracing::info!("Adding missing level field to log config (default: info)");
                        needs_save = true;
                    }
                }
                // Ensure memorySystem config section exists
                if !raw.contains("memorySystem") {
                    tracing::info!(
                        "Adding missing memorySystem config section to config (7-layer memory thresholds)"
                    );
                    needs_save = true;
                }
            }
        }

        // Detect timezone if not set (only for existing configs with missing field)
        let config = if config.default_timezone.is_none() {
            // Only reached for existing configs with missing default_timezone
            let detected_tz = detect_system_timezone();
            if let Some(ref tz) = detected_tz {
                tracing::info!(timezone = %tz, "Setting detected timezone in config");
            }
            Config {
                default_timezone: detected_tz,
                ..config
            }
        } else {
            config
        };

        // Save if we added missing fields
        if needs_save || !config_path.exists() {
            if let Err(e) = config.save(&config_path) {
                tracing::warn!(error = %e, "Failed to save updated config file");
            } else {
                tracing::info!(path = %config_path.display(), "Config file updated with missing fields");
            }
        }

        // Validate memorySystem config and log warnings (non-fatal)
        let mem_warnings = config.memory.memory_system.validate();
        for warning in &mem_warnings {
            tracing::warn!(warning, "memorySystem config warning");
        }

        Ok(config)
    }

    /// Validate config values and return self if valid.
    fn validate(self) -> Result<Self> {
        // Validate cron_tick_interval_secs
        if self.cron_tick_interval_secs < MIN_CRON_TICK_INTERVAL_SECS {
            tracing::warn!(
                value = self.cron_tick_interval_secs,
                min = MIN_CRON_TICK_INTERVAL_SECS,
                "cron_tick_interval_secs too small, using minimum value"
            );
            return Err(crate::Error::Config(format!(
                "cron_tick_interval_secs must be at least {} seconds, got {}",
                MIN_CRON_TICK_INTERVAL_SECS, self.cron_tick_interval_secs
            )));
        }
        if self.cron_tick_interval_secs > MAX_CRON_TICK_INTERVAL_SECS {
            tracing::warn!(
                value = self.cron_tick_interval_secs,
                max = MAX_CRON_TICK_INTERVAL_SECS,
                "cron_tick_interval_secs too large, using maximum value"
            );
            return Err(crate::Error::Config(format!(
                "cron_tick_interval_secs must be at most {} seconds, got {}",
                MAX_CRON_TICK_INTERVAL_SECS, self.cron_tick_interval_secs
            )));
        }

        // Validate default_timezone if set
        if let Some(ref tz) = self.default_timezone {
            if tz.parse::<chrono_tz::Tz>().is_err() {
                return Err(crate::Error::Config(format!(
                    "Invalid default_timezone '{}'. Use IANA timezone like 'Asia/Shanghai', 'America/New_York'",
                    tz
                )));
            }
        }

        Ok(self)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_json5_pretty(path, self)
    }

    pub fn get_api_key(&self) -> Option<(&str, &ProviderConfig)> {
        let priority = [
            "openrouter",
            "deepseek",
            "anthropic",
            "openai",
            "kimi",
            "gemini",
            "zhipu",
            "groq",
            "vllm",
            "ollama",
        ];

        for name in priority {
            if let Some(provider) = self.providers.get(name) {
                if !provider.api_key.is_empty() {
                    return Some((name, provider));
                }
            }
        }
        None
    }

    pub fn get_provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    pub fn community_hub_url(&self) -> Option<String> {
        if let Some(url) = self.community_hub.hub_url.as_ref() {
            let url = url.trim();
            if !url.is_empty() {
                return Some(url.trim_end_matches('/').to_string());
            }
        }
        None
    }

    pub fn community_hub_api_key(&self) -> Option<String> {
        if let Some(key) = self.community_hub.api_key.as_ref() {
            let key = key.trim();
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
        None
    }

    pub fn resolve_channel_owner(&self, channel: &str) -> Option<&str> {
        self.channel_owners
            .get(channel)
            .map(|owner| owner.as_str())
            .filter(|owner| !owner.trim().is_empty())
    }

    pub fn resolve_channel_account_owner(&self, channel: &str, account_id: &str) -> Option<&str> {
        let account_id = account_id.trim();
        if account_id.is_empty() {
            return None;
        }

        self.channel_account_owners
            .get(channel)
            .and_then(|owners| owners.get(account_id))
            .map(|owner| owner.as_str())
            .filter(|owner| !owner.trim().is_empty())
    }

    pub fn resolve_effective_channel_owner(
        &self,
        channel: &str,
        account_id: Option<&str>,
    ) -> Option<&str> {
        account_id
            .and_then(|account_id| self.resolve_channel_account_owner(channel, account_id))
            .or_else(|| self.resolve_channel_owner(channel))
    }

    pub fn is_external_channel_enabled(&self, channel: &str) -> bool {
        match channel {
            "telegram" => self.channels.telegram.enabled,
            "whatsapp" => self.channels.whatsapp.enabled,
            "feishu" => self.channels.feishu.enabled,
            "slack" => self.channels.slack.enabled,
            "discord" => self.channels.discord.enabled,
            "dingtalk" => self.channels.dingtalk.enabled,
            "wecom" => self.channels.wecom.enabled,
            "lark" => self.channels.lark.enabled,
            "qq" => self.channels.qq.enabled,
            "napcat" => self.channels.napcat.enabled,
            "weixin" => self.channels.weixin.enabled,
            _ => false,
        }
    }

    pub fn known_agent_ids(&self) -> Vec<String> {
        let mut ids = vec!["default".to_string()];
        for agent in self.agents.list.iter().filter(|agent| agent.enabled) {
            let agent_id = agent.id.trim();
            if agent_id.is_empty() || agent_id == "default" {
                continue;
            }
            if !ids.iter().any(|id| id == agent_id) {
                ids.push(agent_id.to_string());
            }
        }
        ids
    }

    pub fn agent_exists(&self, agent_id: &str) -> bool {
        let agent_id = agent_id.trim();
        !agent_id.is_empty() && self.known_agent_ids().iter().any(|id| id == agent_id)
    }

    pub fn resolve_agent_spec(&self, agent_id: &str) -> Option<ResolvedAgentConfig> {
        let agent_id = agent_id.trim();
        if agent_id.is_empty() {
            return None;
        }

        let agent = self
            .agents
            .list
            .iter()
            .find(|agent| agent.enabled && agent.id.trim() == agent_id);

        if agent_id != "default" && agent.is_none() {
            return None;
        }

        let mut defaults = self.agents.defaults.clone();
        if let Some(agent) = agent {
            let explicit_model = agent
                .model
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let explicit_provider = agent
                .provider
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let has_single_model_override = explicit_model.is_some() || explicit_provider.is_some();

            if let Some(model) = explicit_model {
                defaults.model = model;
            }
            if let Some(provider) = explicit_provider {
                defaults.provider = Some(provider);
            }
            if !agent.model_pool.is_empty() {
                defaults.model_pool = agent.model_pool.clone();
            } else if has_single_model_override {
                defaults.model_pool.clear();
            }
            if let Some(max_tokens) = agent.max_tokens {
                defaults.max_tokens = max_tokens;
            }
            if let Some(temperature) = agent.temperature {
                defaults.temperature = temperature;
            }
            if let Some(max_tool_iterations) = agent.max_tool_iterations {
                defaults.max_tool_iterations = max_tool_iterations;
            }
            if !agent.max_tool_iterations_by_tool.is_empty() {
                defaults.max_tool_iterations_by_tool = agent.max_tool_iterations_by_tool.clone();
            }
            if let Some(llm_max_retries) = agent.llm_max_retries {
                defaults.llm_max_retries = llm_max_retries;
            }
            if let Some(llm_retry_delay_ms) = agent.llm_retry_delay_ms {
                defaults.llm_retry_delay_ms = llm_retry_delay_ms;
            }
            if let Some(max_context_tokens) = agent.max_context_tokens {
                defaults.max_context_tokens = max_context_tokens;
            }
            if let Some(evolution_model) = agent
                .evolution_model
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
            {
                defaults.evolution_model = Some(evolution_model);
            }
            if let Some(evolution_provider) = agent
                .evolution_provider
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
            {
                defaults.evolution_provider = Some(evolution_provider);
            }
            if let Some(allowed_mcp_servers) = &agent.allowed_mcp_servers {
                defaults.allowed_mcp_servers = allowed_mcp_servers.clone();
            }
            if let Some(allowed_mcp_tools) = &agent.allowed_mcp_tools {
                defaults.allowed_mcp_tools = allowed_mcp_tools.clone();
            }
        }

        Some(ResolvedAgentConfig {
            id: agent_id.to_string(),
            name: agent.and_then(|entry| entry.name.clone()),
            defaults,
            intent_profile: self.resolve_intent_profile_id(Some(agent_id)),
        })
    }

    pub fn resolved_agents(&self) -> Vec<ResolvedAgentConfig> {
        self.known_agent_ids()
            .into_iter()
            .filter_map(|agent_id| self.resolve_agent_spec(&agent_id))
            .collect()
    }

    pub fn config_for_agent(&self, agent_id: &str) -> Option<Config> {
        let resolved = self.resolve_agent_spec(agent_id)?;
        let mut config = self.clone();
        config.agents.defaults = resolved.defaults;
        Some(config)
    }

    pub fn resolve_intent_profile_id(&self, agent_id: Option<&str>) -> Option<String> {
        let router = self.intent_router.clone().unwrap_or_default();

        let requested_agent_id = agent_id.map(str::trim).filter(|id| !id.is_empty());

        if let Some(agent_id) = requested_agent_id {
            if let Some(profile) = self
                .agents
                .list
                .iter()
                .find(|agent| agent.enabled && agent.id.trim() == agent_id)
                .and_then(|agent| agent.intent_profile.as_deref())
                .map(str::trim)
                .filter(|profile| !profile.is_empty())
            {
                return Some(profile.to_string());
            }

            if let Some(profile) = router
                .agent_profiles
                .get(agent_id)
                .map(String::as_str)
                .map(str::trim)
                .filter(|profile| !profile.is_empty())
            {
                return Some(profile.to_string());
            }
        }

        let default_profile = router.default_profile.trim();
        if default_profile.is_empty() {
            None
        } else {
            Some(default_profile.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("blockcell-config-tests-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        dir.join(name)
    }

    #[test]
    fn test_config_load_accepts_json5_comments_and_trailing_commas() {
        let path = temp_config_path("config.json5");
        fs::write(
            &path,
            r#"{
  // provider config in JSON5
  providers: {
    openai: {
      apiKey: 'sk-test',
    },
  },
  agents: {
    defaults: {
      model: 'gpt-4.1',
    },
  },
}"#,
        )
        .expect("write config.json5");

        let cfg = Config::load(&path).expect("load json5 config");
        assert_eq!(cfg.agents.defaults.model, "gpt-4.1");
        assert_eq!(
            cfg.providers.get("openai").map(|p| p.api_key.as_str()),
            Some("sk-test")
        );
    }

    #[test]
    fn test_embedded_ghost_learning_defaults_are_safe() {
        let cfg = GhostConfig::default();
        assert!(cfg.learning.enabled);
        assert!(cfg.learning.shadow_mode);
        assert_eq!(cfg.learning.recall_max_items, 4);
    }

    #[test]
    fn test_config_save_round_trips_via_json5_loader() {
        let path = temp_config_path("config.json5");
        let mut cfg = Config::default();
        cfg.agents.defaults.model = "deepseek-chat".to_string();
        cfg.memory.vector.enabled = true;
        cfg.memory.vector.provider = "openai".to_string();
        cfg.memory.vector.model = "text-embedding-3-small".to_string();
        cfg.memory.vector.uri = Some("./memory/vectors.rabitq".to_string());

        cfg.save(&path).expect("save config json5");
        let content = fs::read_to_string(&path).expect("read saved config");
        assert!(content.contains("deepseek-chat"));
        assert!(content.contains("text-embedding-3-small"));

        let loaded = Config::load(&path).expect("reload saved config");
        assert_eq!(loaded.agents.defaults.model, "deepseek-chat");
        assert!(loaded.memory.vector.enabled);
        assert_eq!(loaded.memory.vector.provider, "openai");
        assert_eq!(loaded.memory.vector.model, "text-embedding-3-small");
        assert_eq!(
            loaded.memory.vector.uri.as_deref(),
            Some("./memory/vectors.rabitq")
        );
    }

    #[test]
    fn test_config_load_expands_env_vars_in_json5() {
        let path = temp_config_path("config.json5");
        // SAFETY: This test runs in a single-threaded context. The environment
        // variable modification is isolated to this test's scope and will be
        // cleaned up at the end of the test. No other threads access these vars.
        unsafe {
            std::env::set_var("BLOCKCELL_TEST_OPENAI_KEY", "sk-from-env");
            std::env::remove_var("BLOCKCELL_TEST_MODEL");
        }

        fs::write(
            &path,
            r#"{
  providers: {
    openai: {
      apiKey: "${BLOCKCELL_TEST_OPENAI_KEY}",
    },
  },
  agents: {
    defaults: {
      model: "${BLOCKCELL_TEST_MODEL:-gpt-4.1}",
    },
  },
}"#,
        )
        .expect("write config.json5");

        let cfg = Config::load(&path).expect("load env-expanded json5 config");
        assert_eq!(
            cfg.providers.get("openai").map(|p| p.api_key.as_str()),
            Some("sk-from-env")
        );
        assert_eq!(cfg.agents.defaults.model, "gpt-4.1");

        unsafe {
            std::env::remove_var("BLOCKCELL_TEST_OPENAI_KEY");
            std::env::remove_var("BLOCKCELL_TEST_MODEL");
        }
    }

    #[test]
    fn test_config_loads_memory_vector_config() {
        let raw = r#"{
  providers: {
    openai: {
      apiKey: "sk-test"
    }
  },
  memory: {
    vector: {
      enabled: true,
      provider: "openai",
      model: "text-embedding-3-small",
      uri: "./memory/rabitq",
      table: "memory_vectors"
    }
  }
}"#;

        let cfg: Config = json5::from_str(raw).expect("parse config");
        assert!(cfg.memory.vector.enabled);
        assert_eq!(cfg.memory.vector.provider, "openai");
        assert_eq!(cfg.memory.vector.model, "text-embedding-3-small");
        assert_eq!(cfg.memory.vector.uri.as_deref(), Some("./memory/rabitq"));
        assert_eq!(cfg.memory.vector.table, "memory_vectors");
    }

    #[test]
    fn test_memory_circuit_breaker_tracks_explicit_presence() {
        let omitted_raw = r#"{
  "memory": {
    "memorySystem": {}
  }
}"#;
        let omitted: Config = serde_json::from_str(omitted_raw).unwrap();
        assert!(!omitted.memory.memory_system.circuit_breaker.is_configured());

        let explicit_default_raw = r#"{
  "memory": {
    "memorySystem": {
      "circuitBreaker": {
        "maxFailures": 3,
        "resetTimeoutSecs": 60
      }
    }
  }
}"#;
        let explicit_default: Config = serde_json::from_str(explicit_default_raw).unwrap();
        assert!(explicit_default
            .memory
            .memory_system
            .circuit_breaker
            .is_configured());
        assert_eq!(
            explicit_default
                .memory
                .memory_system
                .circuit_breaker
                .reset_timeout_secs,
            60
        );

        let implicit_json = serde_json::to_string(&omitted.memory.memory_system).unwrap();
        assert!(!implicit_json.contains("circuitBreaker"));

        let explicit_json = serde_json::to_string(&explicit_default.memory.memory_system).unwrap();
        assert!(explicit_json.contains("circuitBreaker"));
    }

    #[test]
    fn test_community_hub_top_level() {
        let raw = r#"{
  "communityHub": { "hubUrl": "http://example.com/", "apiKey": "k" },
  "providers": {}
}"#;
        let cfg: Config = serde_json::from_str(raw).unwrap();
        assert_eq!(
            cfg.community_hub_url().as_deref(),
            Some("http://example.com")
        );
        assert_eq!(cfg.community_hub_api_key().as_deref(), Some("k"));
    }

    #[test]
    fn test_channel_owners_and_accounts_deserialize() {
        let raw = r#"{
  "agents": {
    "list": [
      { "id": "chat", "enabled": true }
    ]
  },
  "channelOwners": {
    "telegram": "chat"
  },
  "channels": {
    "telegram": {
      "enabled": true,
      "defaultAccountId": "default",
      "accounts": {
        "default": {
          "enabled": true,
          "token": "tg-token"
        }
      }
    }
  }
}"#;
        let cfg: Config = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.resolve_channel_owner("telegram"), Some("chat"));
        assert!(cfg.is_external_channel_enabled("telegram"));
        assert_eq!(
            cfg.channels.telegram.default_account_id.as_deref(),
            Some("default")
        );
        let acc = cfg.channels.telegram.accounts.get("default").unwrap();
        assert_eq!(acc.token, "tg-token");
        assert!(cfg.agent_exists("chat"));
    }

    #[test]
    fn test_channel_account_owner_override_deserializes_and_resolves() {
        let raw = r#"{
  "agents": {
    "list": [
      { "id": "ops", "enabled": true }
    ]
  },
  "channelOwners": {
    "telegram": "default"
  },
  "channelAccountOwners": {
    "telegram": {
      "bot2": "ops"
    }
  },
  "channels": {
    "telegram": {
      "enabled": true,
      "accounts": {
        "bot1": { "enabled": true, "token": "tg-bot1" },
        "bot2": { "enabled": true, "token": "tg-bot2" }
      }
    }
  }
}"#;
        let cfg: Config = serde_json::from_str(raw).unwrap();

        assert_eq!(
            cfg.resolve_channel_account_owner("telegram", "bot2"),
            Some("ops")
        );
        assert_eq!(
            cfg.resolve_effective_channel_owner("telegram", Some("bot2")),
            Some("ops")
        );
        assert_eq!(
            cfg.resolve_effective_channel_owner("telegram", Some("bot1")),
            Some("default")
        );
        assert_eq!(
            cfg.resolve_effective_channel_owner("telegram", None),
            Some("default")
        );
    }

    #[test]
    fn test_channel_account_owner_resolution_ignores_blank_values() {
        let raw = r#"{
  "channelOwners": {
    "telegram": "default"
  },
  "channelAccountOwners": {
    "telegram": {
      "bot1": "   "
    }
  }
}"#;
        let cfg: Config = serde_json::from_str(raw).unwrap();

        assert_eq!(cfg.resolve_channel_account_owner("telegram", "bot1"), None);
        assert_eq!(
            cfg.resolve_effective_channel_owner("telegram", Some("bot1")),
            Some("default")
        );
    }

    #[test]
    fn test_legacy_single_channel_fields_still_work() {
        let raw = r#"{
  "channels": {
    "telegram": {
      "enabled": true,
      "token": "legacy-token"
    }
  }
}"#;
        let cfg: Config = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.channels.telegram.token, "legacy-token");
        assert!(cfg.channels.telegram.accounts.is_empty());
        assert_eq!(cfg.channels.telegram.default_account_id, None);
        assert!(cfg.agent_exists("default"));
    }

    #[test]
    fn test_known_agent_ids_fallback_to_default() {
        let cfg = Config::default();
        let ids = cfg.known_agent_ids();
        assert_eq!(ids, vec!["default".to_string()]);
    }

    #[test]
    fn test_intent_router_deserializes_and_resolves_agent_profile() {
        let raw = r#"{
  "agents": {
    "list": [
      { "id": "ops", "enabled": true, "intentProfile": "ops" }
    ]
  },
  "intentRouter": {
    "enabled": true,
    "defaultProfile": "default",
    "profiles": {
      "default": {
        "coreTools": ["read_file", "message"],
        "intentTools": {
          "Chat": { "inheritBase": false, "tools": [] },
          "Unknown": ["browse"]
        }
      },
      "ops": {
        "coreTools": ["read_file", "exec"],
        "intentTools": {
          "DevOps": ["git_api"],
          "Unknown": ["http_request"]
        },
        "denyTools": ["email"]
      }
    }
  }
}"#;

        let cfg: Config = serde_json::from_str(raw).unwrap();
        let router = cfg.intent_router.as_ref().expect("intent router");
        assert!(router.enabled);
        assert_eq!(
            cfg.resolve_intent_profile_id(Some("ops")),
            Some("ops".to_string())
        );
        assert_eq!(
            cfg.resolve_intent_profile_id(Some("missing")),
            Some("default".to_string())
        );
        assert_eq!(
            cfg.resolve_intent_profile_id(None),
            Some("default".to_string())
        );
    }

    #[test]
    fn test_default_config_includes_intent_router_defaults() {
        let cfg = Config::default();
        let router = cfg.intent_router.as_ref().expect("default intent router");

        assert!(router.profiles.contains_key("default"));
        assert_eq!(
            cfg.resolve_intent_profile_id(Some("default")),
            Some("default".to_string())
        );
    }

    #[test]
    fn test_missing_intent_router_uses_default_router() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        let router = cfg.intent_router.as_ref().expect("defaulted intent router");

        assert!(router.enabled);
        assert!(router.profiles.contains_key("default"));
        assert_eq!(
            cfg.resolve_intent_profile_id(None),
            Some("default".to_string())
        );
    }

    #[test]
    fn test_resolved_agent_falls_back_to_implicit_default() {
        let cfg = Config::default();
        let resolved = cfg
            .resolve_agent_spec("default")
            .expect("implicit default agent");

        assert_eq!(resolved.id, "default");
        assert_eq!(resolved.defaults.model, cfg.agents.defaults.model);
        assert_eq!(resolved.defaults.provider, cfg.agents.defaults.provider);
        assert_eq!(resolved.intent_profile.as_deref(), Some("default"));
    }

    #[test]
    fn test_resolved_agent_inherits_and_overrides_defaults() {
        let raw = r#"{
  "agents": {
    "defaults": {
      "model": "deepseek-chat",
      "provider": "deepseek",
      "modelPool": [
        { "model": "deepseek-chat", "provider": "deepseek", "weight": 1, "priority": 1 }
      ]
    },
    "list": [
      {
        "id": "ops",
        "enabled": true,
        "intentProfile": "ops",
        "model": "gpt-4.1",
        "provider": "openai"
      }
    ]
  },
  "intentRouter": {
    "enabled": true,
    "defaultProfile": "default",
    "profiles": {
      "default": {
        "coreTools": ["read_file"],
        "intentTools": { "Unknown": [] }
      },
      "ops": {
        "coreTools": ["exec"],
        "intentTools": { "Unknown": ["http_request"] }
      }
    }
  }
}"#;

        let cfg: Config = serde_json::from_str(raw).unwrap();
        let resolved = cfg.resolve_agent_spec("ops").expect("resolved ops agent");

        assert_eq!(resolved.id, "ops");
        assert_eq!(resolved.defaults.model, "gpt-4.1");
        assert_eq!(resolved.defaults.provider.as_deref(), Some("openai"));
        assert!(
            resolved.defaults.model_pool.is_empty(),
            "explicit model/provider override should disable inherited model_pool"
        );
        assert_eq!(resolved.intent_profile.as_deref(), Some("ops"));
    }

    #[test]
    fn test_resolved_agents_always_include_default() {
        let raw = r#"{
  "agents": {
    "list": [
      { "id": "ops", "enabled": true, "intentProfile": "ops" }
    ]
  },
  "intentRouter": {
    "enabled": true,
    "defaultProfile": "default",
    "profiles": {
      "default": {
        "coreTools": ["read_file"],
        "intentTools": { "Unknown": [] }
      },
      "ops": {
        "coreTools": ["exec"],
        "intentTools": { "Unknown": ["http_request"] }
      }
    }
  }
}"#;

        let cfg: Config = serde_json::from_str(raw).unwrap();
        let ids: Vec<String> = cfg
            .resolved_agents()
            .into_iter()
            .map(|agent| agent.id)
            .collect();
        assert_eq!(ids, vec!["default".to_string(), "ops".to_string()]);
    }
}
