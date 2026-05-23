use blockcell_agent::{
    AgentRuntime, CapabilityRegistryAdapter, CheckpointManager, ConfirmRequest,
    CoreEvolutionAdapter, MemoryStoreAdapter, MessageBus, ProviderLLMBridge, ResponseCache,
    ResponseCacheConfig, SkillEvolutionLLMBridge, TaskManager,
};
#[cfg(feature = "dingtalk")]
use blockcell_channels::dingtalk::DingTalkChannel;
#[cfg(feature = "discord")]
use blockcell_channels::discord::DiscordChannel;
#[cfg(feature = "feishu")]
use blockcell_channels::feishu::FeishuChannel;
#[cfg(feature = "slack")]
use blockcell_channels::slack::SlackChannel;
#[cfg(feature = "telegram")]
use blockcell_channels::telegram::TelegramChannel;
#[cfg(feature = "wecom")]
use blockcell_channels::wecom::WeComChannel;
#[cfg(feature = "whatsapp")]
use blockcell_channels::whatsapp::WhatsAppChannel;
use blockcell_channels::ChannelManager;
use blockcell_core::{Config, InboundMessage, Paths};
use blockcell_providers::{Provider, ProviderPool};
use blockcell_scheduler::{
    CronService, DreamService, DreamServiceConfig, EvolutionWorker, SkillEvolutionWorker,
};
use blockcell_skills::{new_registry_handle, CoreEvolution, EvolutionServiceConfig};
use blockcell_storage::EvolutionWorkflowStore;
use blockcell_tools::mcp::manager::McpManager;
use blockcell_tools::{
    build_tool_registry_for_agent_config, CapabilityRegistryHandle, CoreEvolutionHandle,
    MemoryStoreHandle,
};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{info, warn};
use unicode_segmentation::UnicodeSegmentation;

use super::memory_store::open_memory_store;
use super::slash_commands::{CommandContext, CommandResult, SLASH_COMMAND_HANDLER};

fn create_skill_evolution_llm_provider(
    config: &Config,
    provider_pool: &ProviderPool,
) -> Option<Arc<dyn blockcell_skills::LLMProvider>> {
    let provider: Option<Arc<dyn Provider>> = if config.agents.defaults.evolution_model.is_some()
        || config.agents.defaults.evolution_provider.is_some()
    {
        match super::provider::create_evolution_provider(config) {
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

/// Built-in tools grouped by category for /tools display.
/// This must include ALL tools registered in ToolRegistry::with_defaults().
const BUILTIN_TOOLS: &[(&str, &[(&str, &str)])] = &[
    (
        "📁 Filesystem",
        &[
            ("read_file", "Read files (text/Office/PDF)"),
            ("write_file", "Create and write files"),
            ("edit_file", "Precise file content editing"),
            ("list_dir", "Browse directory structure"),
            ("file_ops", "Delete/move/copy/compress/decompress/PDF"),
        ],
    ),
    (
        "⚡ Commands & System",
        &[
            ("exec", "Execute shell commands"),
            ("system_info", "Hardware/software/network detection"),
        ],
    ),
    (
        "🌐 Web & Browser",
        &[
            ("web_search", "Search engine queries"),
            ("web_fetch", "Fetch web page content"),
            (
                "browse",
                "CDP browser automation (35+ actions, tabs/screenshots/PDF/network)",
            ),
            ("http_request", "Generic HTTP/REST API calls"),
        ],
    ),
    (
        "🖥️ GUI Automation",
        &[("app_control", "macOS app control (System Events)")],
    ),
    (
        "🎨 Media",
        &[
            ("camera_capture", "Camera capture"),
            ("audio_transcribe", "Speech-to-text (Whisper/API)"),
            ("tts", "Text-to-speech (say/piper/edge-tts/OpenAI)"),
            ("ocr", "Image text recognition (Tesseract/Vision/API)"),
            (
                "image_understand",
                "Multimodal image understanding (GPT-4o/Claude/Gemini)",
            ),
            (
                "video_process",
                "Video processing (ffmpeg cut/merge/subtitle/watermark/compress)",
            ),
            ("chart_generate", "Chart generation (matplotlib/plotly)"),
        ],
    ),
    (
        "📊 Data Processing",
        &[
            ("data_process", "CSV read/write/stats/query/transform"),
            ("office_write", "Generate PPTX/DOCX/XLSX documents"),
            (
                "knowledge_graph",
                "Knowledge graph (entities/relations/paths/export DOT/Mermaid)",
            ),
        ],
    ),
    (
        "📬 Communication",
        &[
            ("email", "Email send/receive (SMTP/IMAP, attachments)"),
            ("message", "Channel messaging (Telegram/Slack/Discord)"),
        ],
    ),
    (
        "💬 NapCatQQ - User",
        &[
            ("napcat_get_login_info", "Get bot login account info"),
            ("napcat_get_status", "Get bot online status"),
            ("napcat_get_version_info", "Get NapCat version info"),
            ("napcat_get_stranger_info", "Get stranger user info"),
            ("napcat_get_friend_list", "Get friend list"),
            ("napcat_send_like", "Send like to user"),
            ("napcat_set_friend_remark", "Set friend remark"),
            ("napcat_delete_friend", "Delete friend"),
            ("napcat_set_qq_profile", "Set bot profile"),
        ],
    ),
    (
        "💬 NapCatQQ - Group",
        &[
            ("napcat_get_group_list", "Get list of joined groups"),
            ("napcat_get_group_info", "Get group detailed info"),
            ("napcat_get_group_member_list", "Get group member list"),
            ("napcat_get_group_member_info", "Get group member info"),
            ("napcat_set_group_kick", "Kick group member"),
            ("napcat_set_group_ban", "Ban group member"),
            ("napcat_set_group_whole_ban", "Set group whole ban"),
            ("napcat_set_group_admin", "Set group admin"),
            ("napcat_set_group_card", "Set group card"),
            ("napcat_set_group_name", "Set group name"),
            ("napcat_set_group_special_title", "Set group special title"),
            ("napcat_set_group_leave", "Leave group"),
        ],
    ),
    (
        "💬 NapCatQQ - Message",
        &[
            ("napcat_delete_msg", "Recall/delete message"),
            ("napcat_get_msg", "Get message by ID"),
            ("napcat_set_friend_add_request", "Handle friend add request"),
            ("napcat_set_group_add_request", "Handle group add request"),
            ("napcat_get_cookies", "Get cookies"),
            ("napcat_get_csrf_token", "Get CSRF token"),
        ],
    ),
    (
        "💬 NapCatQQ - Extend",
        &[
            ("napcat_get_forward_msg", "Get forwarded message content"),
            ("napcat_set_msg_emoji_like", "Set emoji reaction"),
            ("napcat_mark_msg_as_read", "Mark message as read"),
            ("napcat_set_essence_msg", "Set essence message"),
            ("napcat_delete_essence_msg", "Delete essence message"),
            ("napcat_get_essence_msg_list", "Get essence message list"),
            (
                "napcat_get_group_at_all_remain",
                "Get group @all remain count",
            ),
            ("napcat_get_image", "Get image from message"),
            ("napcat_get_record", "Get voice record from message"),
            ("napcat_download_file", "Download file"),
        ],
    ),
    ("📅 Business Integration", &[]),
    (
        "💰 Finance",
        &[
            (
                "stream_subscribe",
                "Real-time data streams (WebSocket/SSE, CEX feeds)",
            ),
            (
                "alert_rule",
                "Conditional monitoring alerts (price/indicator/change rate)",
            ),
        ],
    ),
    ("⛓️ Blockchain", &[]),
    (
        "🔒 Security & Network",
        &[
            ("encrypt", "Encrypt/decrypt/password/hash/encode"),
            (
                "network_monitor",
                "Network diagnostics (ping/traceroute/port scan/SSL/DNS/WHOIS)",
            ),
        ],
    ),
    (
        "🧠 Memory & Cognition",
        &[
            ("memory_query", "Full-text memory search (SQLite FTS5)"),
            ("memory_upsert", "Structured memory storage"),
            ("memory_forget", "Memory delete and restore"),
        ],
    ),
    (
        "🤖 Autonomy & Evolution",
        &[
            ("spawn", "Spawn sub-agents for parallel execution"),
            ("list_tasks", "View task status"),
            ("cron", "Scheduled task management"),
            ("list_skills", "Skill learning status query"),
            ("capability_evolve", "Self-learn new tools via evolution"),
        ],
    ),
];

/// Extract image file paths from user input.
/// Supports:
/// - Inline absolute paths: `/path/to/image.png what is this image`
/// - @-prefixed paths: `@/path/to/image.png recognize this`
/// - ~ home dir paths: `~/Desktop/photo.jpg take a look`
///
/// Returns (cleaned_text, media_paths).
fn extract_media_from_input(input: &str) -> (String, Vec<String>) {
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
fn create_provider(config: &Config) -> anyhow::Result<Box<dyn Provider>> {
    super::provider::create_provider(config)
}

fn build_pool_with_overrides(
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
struct AgentCliContext {
    agent_id: String,
    session: String,
    config: Config,
    paths: Paths,
}

fn resolve_agent_context(
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

pub async fn run(
    message: Option<String>,
    agent: Option<String>,
    session: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> anyhow::Result<()> {
    let root_paths = Paths::new();
    super::env_file::ensure_and_load_blockcell_env(&root_paths)?;
    let root_config = Config::load_or_default(&root_paths)?;
    let resolved = resolve_agent_context(
        &root_config,
        &root_paths,
        agent.as_deref(),
        session.as_deref(),
    )?;
    let agent_id = resolved.agent_id.clone();
    let session = resolved.session;
    let paths = resolved.paths;
    paths.ensure_dirs()?;
    let mut config = resolved.config;
    let mcp_manager = Arc::new(McpManager::load(&root_paths).await?);
    let provider_pool = build_pool_with_overrides(&mut config, model, provider)?;

    // Ensure builtin skills are extracted to workspace/skills/ (silent, skips existing)
    let _ = super::embedded_skills::extract_to_workspace(&paths.skills_dir());

    // Initialize memory store (SQLite + FTS5)
    let memory_store_handle: Option<MemoryStoreHandle> = match open_memory_store(&paths, &config) {
        Ok(store) => {
            // Run migration from MEMORY.md/daily files on first startup
            if let Err(e) = store.migrate_from_files(&paths.memory_dir()) {
                eprintln!("Warning: memory migration failed: {}", e);
            }
            let adapter = MemoryStoreAdapter::new(store);
            Some(Arc::new(adapter))
        }
        Err(e) => {
            eprintln!(
                "Warning: failed to open memory store: {}. Memory tools will be unavailable.",
                e
            );
            None
        }
    };

    // Initialize tool evolution registry and core evolution engine
    let cap_registry_dir = paths.evolved_tools_dir();
    let cap_registry_raw = new_registry_handle(cap_registry_dir);
    {
        let mut reg = cap_registry_raw.lock().await;
        let _ = reg.load(); // Load persisted evolved tools from disk
        let rehydrated = reg.rehydrate_executors(); // Rebuild executors for persisted evolved tools
        if rehydrated > 0 {
            info!("Rehydrated {} evolved tool executors from disk", rehydrated);
        }
    }

    // 使用配置中的 LLM 超时设置，默认 300 秒
    let llm_timeout_secs = 300u64;
    let mut core_evo = CoreEvolution::new(
        paths.workspace().to_path_buf(),
        cap_registry_raw.clone(),
        llm_timeout_secs,
    );

    // Create an LLM provider bridge so CoreEvolution can generate code autonomously
    if let Some((_, evo_p)) = provider_pool.acquire() {
        let llm_bridge = Arc::new(ProviderLLMBridge::new_arc(evo_p));
        core_evo.set_llm_provider(llm_bridge);
        info!("Core evolution LLM provider configured");
    }

    let core_evo_raw = Arc::new(Mutex::new(core_evo));

    // Create adapter handles for the tools crate trait objects
    let cap_registry_adapter = CapabilityRegistryAdapter::new(cap_registry_raw.clone());
    let cap_registry_handle: CapabilityRegistryHandle = Arc::new(Mutex::new(cap_registry_adapter));

    let core_evo_adapter = CoreEvolutionAdapter::new(core_evo_raw.clone());
    let core_evo_handle: CoreEvolutionHandle = Arc::new(Mutex::new(core_evo_adapter));

    // 创建核心进化工作流存储和 worker（在 if/else 之前创建，两个分支都需要）
    let evo_workflow_db = paths.workspace().join("evo_workflow.db");
    let evo_workflow_store = EvolutionWorkflowStore::open(&evo_workflow_db)?;
    let evo_workflow_store_arc = Arc::new(evo_workflow_store);
    let evo_worker = EvolutionWorker::new((*evo_workflow_store_arc).clone(), core_evo_raw.clone());
    let evo_worker_arc = Arc::new(evo_worker);

    let skill_evo_llm_provider = create_skill_evolution_llm_provider(&config, &provider_pool);
    let skill_evo_workflow_db = paths.workspace().join("skill_evolution_workflow.db");
    let skill_evo_workflow_store = EvolutionWorkflowStore::open(&skill_evo_workflow_db)?;
    // 从 Config.evolution 转换配置，而非使用默认值
    let skill_evo_config: EvolutionServiceConfig = config.evolution.clone().into();
    let mut skill_evo_worker = SkillEvolutionWorker::new(
        skill_evo_workflow_store,
        paths.skills_dir(),
        skill_evo_config,
        skill_evo_llm_provider,
    );
    // 为 SkillEvolutionWorker 设置部署回调，确保 scheduler worker 的进化部署路径也能触发 ghost learning boundary
    if let Some(callback) = blockcell_agent::create_evolution_deploy_callback(&config, &paths) {
        skill_evo_worker.set_deploy_callback(callback);
        info!("[evolution-deploy-callback] 已连接到 SkillEvolutionWorker EvolutionService");
    }
    let skill_evo_worker_arc = Arc::new(skill_evo_worker);

    if let Some(msg) = message {
        // Single message mode — no need for CronService
        let tool_registry =
            build_tool_registry_for_agent_config(&config, Some(&mcp_manager)).await?;
        let mut runtime = AgentRuntime::new(
            config.clone(),
            paths.clone(),
            Arc::clone(&provider_pool),
            tool_registry,
        )?;
        runtime.validate_intent_router()?;
        runtime.set_agent_id(Some(agent_id.clone()));
        runtime.set_task_manager(TaskManager::new());

        // 如果配置了独立的 evolution_model 或 evolution_provider，创建独立的 evolution provider
        if config.agents.defaults.evolution_model.is_some()
            || config.agents.defaults.evolution_provider.is_some()
        {
            match super::provider::create_evolution_provider(&config) {
                Ok(evo_provider) => {
                    runtime.set_evolution_provider(evo_provider);
                    info!("Evolution provider configured with independent model");
                }
                Err(e) => {
                    warn!(
                        "Failed to create evolution provider: {}, using main provider",
                        e
                    );
                }
            }
        }

        if let Some(ref store) = memory_store_handle {
            runtime.set_memory_store(store.clone());
        }
        if let Err(e) = runtime.init_memory_file_store() {
            warn!(error = %e, "Failed to initialize file memory store");
        }
        if let Err(e) = runtime.init_skill_file_store() {
            warn!(error = %e, "Failed to initialize skill file store");
        }

        runtime.set_capability_registry(cap_registry_handle.clone());
        runtime.set_core_evolution(core_evo_handle.clone());

        // 设置核心进化工作流存储和 worker
        runtime.set_evolution_workflow_store(evo_workflow_store_arc.clone());
        runtime.set_evolution_worker(
            evo_worker_arc.clone() as Arc<dyn blockcell_agent::EvolutionNotifier>
        );
        runtime.set_skill_evolution_worker(
            skill_evo_worker_arc.clone() as Arc<dyn blockcell_agent::EvolutionNotifier>
        );

        // Initialize Layer 5 memory injector (7-layer memory system)
        if let Err(e) = runtime.init_memory_injector().await {
            warn!(error = %e, "Failed to initialize memory injector");
        }

        // Create event broadcast channel for streaming output
        // 容量 2048：避免长 streaming 响应（大量 token 事件）导致 receiver Lagged
        let (event_tx, mut event_rx) = broadcast::channel::<String>(2048);
        runtime.set_event_tx(event_tx.clone());

        // Spawn event handler for streaming token output
        let event_handler = tokio::spawn(async move {
            use std::io::Write;
            let mut stdout = std::io::stdout();
            let mut emitted_text_delta = false;
            let mut emitted_thinking = false;
            loop {
                match event_rx.recv().await {
                    Ok(event_str) => {
                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(&event_str) {
                            let event_type =
                                event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match event_type {
                                "token" => {
                                    if let Some(delta) = event.get("delta").and_then(|v| v.as_str())
                                    {
                                        // 在 thinking 之后、第一个 token 之前插入换行
                                        if emitted_thinking {
                                            println!();
                                            emitted_thinking = false;
                                        }
                                        emitted_text_delta = true;
                                        print!("{}", delta);
                                        let _ = stdout.flush();
                                    }
                                }
                                "thinking" => {
                                    if let Some(content) =
                                        event.get("content").and_then(|v| v.as_str())
                                    {
                                        emitted_thinking = true;
                                        print!("{}", content);
                                        let _ = stdout.flush();
                                    }
                                }
                                "tool_call_start" => {
                                    if let Some(tool) = event.get("tool").and_then(|v| v.as_str()) {
                                        eprintln!("\n🔧 Calling tool: {}...", tool);
                                    }
                                }
                                "message_done" => {
                                    if !emitted_text_delta {
                                        if let Some(content) =
                                            event.get("content").and_then(|v| v.as_str())
                                        {
                                            if !content.is_empty() {
                                                println!("\n{}", content);
                                            }
                                        }
                                    }
                                    println!();
                                    emitted_text_delta = false;
                                    emitted_thinking = false;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // Receiver 落后于发送者，跳过 n 条消息，继续接收
                        tracing::warn!(skipped = n, "Event receiver lagged, skipping messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // 所有 sender 已关闭，退出循环
                        break;
                    }
                }
            }
        });

        let inbound = InboundMessage {
            channel: "cli".to_string(),
            account_id: None,
            sender_id: "user".to_string(),
            chat_id: session.split(':').nth(1).unwrap_or("default").to_string(),
            content: msg,
            media: vec![],
            metadata: serde_json::Value::Null,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        let response = runtime.process_message(inbound).await?;
        // Event handler already printed streaming output, just print final newline if needed
        if !response.is_empty() {
            println!();
        }
        // Clean up event handler
        event_handler.abort();

        // 单次模式驱动 skill evolution pipeline tick，
        // 确保消息处理期间触发的 notify() 有消费者，pipeline 能推进。
        // Bounded loop：单次消息可能触发多个 evolution workflow，drain 到无可 claim 为止。
        {
            let skill_evo_worker_clone = skill_evo_worker_arc.clone();
            let _ = tokio::spawn(async move {
                let max_ticks = 10; // 防止无限循环
                for _ in 0..max_ticks {
                    if !skill_evo_worker_clone.tick().await {
                        break; // 无可 claim 的 workflow，退出
                    }
                }
            })
            .await;
        }
    } else {
        // Interactive mode with CronService
        println!("blockcell interactive mode (Ctrl+C to exit)");
        println!("Agent: {}", agent_id);
        println!("Session: {}", session);
        println!("Type /help to see all available commands.");
        println!();

        // Create message bus
        let bus = MessageBus::new(100);
        let ((inbound_tx, inbound_rx), (outbound_tx, mut outbound_rx)) = bus.split();

        // Create shutdown channel
        let (shutdown_tx, _) = broadcast::channel::<()>(1);

        // Create confirmation channel for path safety checks
        let (confirm_tx, mut confirm_rx) = mpsc::channel::<ConfirmRequest>(8);

        // Create shared task manager with workspace and progress channel for persistence
        let (progress_tx, mut progress_rx) = mpsc::channel::<blockcell_agent::AgentProgress>(100);
        let task_manager =
            TaskManager::with_workspace_and_progress(&paths.workspace(), progress_tx);

        // Restore unfinished tasks from disk
        let restored = task_manager.restore_from_disk(&paths.workspace()).await;
        if restored > 0 {
            info!("Restored {} unfinished tasks from disk", restored);
        }

        // Start periodic cleanup of evicted tasks (with file cleanup)
        let cleanup_handle = Arc::new(task_manager.clone()).spawn_cleanup_loop(&paths.workspace());

        // 启动进度事件监听：在控制台打印任务阶段进度
        tokio::spawn(async move {
            use blockcell_agent::AgentProgress;
            while let Some(progress) = progress_rx.recv().await {
                match progress {
                    AgentProgress::Stage {
                        task_id,
                        stage,
                        percent,
                    } => {
                        let short_id = short_task_id(&task_id, 8);
                        if percent > 0 {
                            eprintln!("[{}] {} ({}%)", short_id, stage, percent);
                        } else {
                            eprintln!("[{}] {}", short_id, stage);
                        }
                    }
                    AgentProgress::Delta { .. } => {
                        // Delta 事件在控制台不打印（太频繁）
                    }
                    AgentProgress::Notification(_) => {
                        // Notification 由其他机制处理
                    }
                    AgentProgress::ToolCallStart { .. } | AgentProgress::ToolCallEnd { .. } => {
                        // 工具调用事件通过 event_tx (broadcast) 处理，此处忽略
                    }
                }
            }
        });

        // Create channel manager for outbound message dispatch (before config is moved)
        let channel_manager =
            ChannelManager::new(config.clone(), paths.clone(), inbound_tx.clone());

        // Start messaging channels (before config is moved into runtime)
        let mut channel_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        #[cfg(feature = "telegram")]
        for listener in blockcell_channels::account::telegram_listener_configs(&config) {
            let telegram = Arc::new(TelegramChannel::new(listener.config, inbound_tx.clone()));
            let shutdown_rx = shutdown_tx.subscribe();
            channel_handles.push(tokio::spawn(async move {
                telegram.run_loop(shutdown_rx).await;
            }));
        }

        #[cfg(feature = "whatsapp")]
        for listener in blockcell_channels::account::whatsapp_listener_configs(&config) {
            let whatsapp = Arc::new(WhatsAppChannel::new(listener.config, inbound_tx.clone()));
            let shutdown_rx = shutdown_tx.subscribe();
            channel_handles.push(tokio::spawn(async move {
                whatsapp.run_loop(shutdown_rx).await;
            }));
        }

        #[cfg(feature = "feishu")]
        for listener in blockcell_channels::account::feishu_scoped_configs(&config) {
            let feishu = Arc::new(FeishuChannel::new(listener.config, inbound_tx.clone()));
            let shutdown_rx = shutdown_tx.subscribe();
            channel_handles.push(tokio::spawn(async move {
                feishu.run_loop(shutdown_rx).await;
            }));
        }

        #[cfg(feature = "slack")]
        for listener in blockcell_channels::account::slack_listener_configs(&config) {
            let slack = Arc::new(SlackChannel::new(listener.config, inbound_tx.clone()));
            let shutdown_rx = shutdown_tx.subscribe();
            channel_handles.push(tokio::spawn(async move {
                slack.run_loop(shutdown_rx).await;
            }));
        }

        #[cfg(feature = "discord")]
        for listener in blockcell_channels::account::discord_listener_configs(&config) {
            let discord = Arc::new(DiscordChannel::new(listener.config, inbound_tx.clone()));
            let shutdown_rx = shutdown_tx.subscribe();
            channel_handles.push(tokio::spawn(async move {
                discord.run_loop(shutdown_rx).await;
            }));
        }

        #[cfg(feature = "dingtalk")]
        for listener in blockcell_channels::account::dingtalk_listener_configs(&config) {
            let dingtalk = Arc::new(DingTalkChannel::new(listener.config, inbound_tx.clone()));
            let shutdown_rx = shutdown_tx.subscribe();
            channel_handles.push(tokio::spawn(async move {
                dingtalk.run_loop(shutdown_rx).await;
            }));
        }

        #[cfg(feature = "wecom")]
        for listener in blockcell_channels::account::wecom_listener_configs(&config) {
            let wecom = Arc::new(WeComChannel::new(listener.config, inbound_tx.clone()));
            let shutdown_rx = shutdown_tx.subscribe();
            channel_handles.push(tokio::spawn(async move {
                wecom.run_loop(shutdown_rx).await;
            }));
        }

        #[cfg(feature = "weixin")]
        for listener in blockcell_channels::account::weixin_listener_configs(&config) {
            let weixin = Arc::new(blockcell_channels::weixin::WeixinChannel::new(
                listener.config,
                inbound_tx.clone(),
            ));
            let shutdown_rx = shutdown_tx.subscribe();
            channel_handles.push(tokio::spawn(async move {
                weixin.run_loop(shutdown_rx).await;
            }));
        }

        // Create agent runtime with outbound channel (consumes config)
        let tool_registry =
            build_tool_registry_for_agent_config(&config, Some(&mcp_manager)).await?;
        let mut runtime = AgentRuntime::new(
            config.clone(),
            paths.clone(),
            Arc::clone(&provider_pool),
            tool_registry,
        )?;
        runtime.validate_intent_router()?;

        // 如果配置了独立的 evolution_model 或 evolution_provider，创建独立的 evolution provider
        if config.agents.defaults.evolution_model.is_some()
            || config.agents.defaults.evolution_provider.is_some()
        {
            match super::provider::create_evolution_provider(&config) {
                Ok(evo_provider) => {
                    runtime.set_evolution_provider(evo_provider);
                    info!("Evolution provider configured with independent model");
                }
                Err(e) => {
                    warn!(
                        "Failed to create evolution provider: {}, using main provider",
                        e
                    );
                }
            }
        }

        // Create event broadcast channel for streaming output
        // 容量 2048：避免长 streaming 响应（大量 token 事件）导致 receiver Lagged
        let (event_tx, mut event_rx) = broadcast::channel::<String>(2048);

        runtime.set_outbound(outbound_tx);
        runtime.set_confirm(confirm_tx);
        runtime.set_task_manager(task_manager.clone());
        runtime.set_agent_id(Some(agent_id.clone()));
        runtime.set_event_tx(event_tx.clone());
        if let Some(ref store) = memory_store_handle {
            runtime.set_memory_store(store.clone());
        }
        if let Err(e) = runtime.init_memory_file_store() {
            warn!(error = %e, "Failed to initialize file memory store");
        }
        if let Err(e) = runtime.init_skill_file_store() {
            warn!(error = %e, "Failed to initialize skill file store");
        }

        runtime.set_capability_registry(cap_registry_handle.clone());
        runtime.set_core_evolution(core_evo_handle.clone());

        // 设置核心进化工作流存储和 worker
        runtime.set_evolution_workflow_store(evo_workflow_store_arc.clone());
        runtime.set_evolution_worker(
            evo_worker_arc.clone() as Arc<dyn blockcell_agent::EvolutionNotifier>
        );
        runtime.set_skill_evolution_worker(
            skill_evo_worker_arc.clone() as Arc<dyn blockcell_agent::EvolutionNotifier>
        );

        // Create shared ResponseCache for CLI and runtime
        // This allows the /clear command to clear the in-memory cache
        let response_cache = ResponseCache::with_config(ResponseCacheConfig::from(
            &config.memory.memory_system.layer1,
        ));
        runtime.set_response_cache(response_cache.clone());

        // Initialize Layer 5 memory injector (7-layer memory system)
        if let Err(e) = runtime.init_memory_injector().await {
            warn!(error = %e, "Failed to initialize memory injector");
        }
        runtime.init_runtime_handle();
        runtime.wire_evolution_deploy_callback();

        let event_emitter = runtime.event_emitter_handle();

        // Create and start CronService
        let tick_interval_secs = config.cron_tick_interval_secs;
        let default_timezone = config.default_timezone.as_deref();
        let cron_service = Arc::new(CronService::new_with_options(
            paths.clone(),
            inbound_tx.clone(),
            Some(agent_id.clone()),
            Some(tick_interval_secs),
            default_timezone,
        ));
        cron_service.set_event_emitter(event_emitter);
        cron_service.load().await?;

        let cron_handle = {
            let cron = cron_service.clone();
            let shutdown_rx = shutdown_tx.subscribe();
            tokio::spawn(async move {
                cron.run_loop(shutdown_rx).await;
            })
        };

        // Layer 6: 启动 Dream Service（跨会话知识整合）
        let dream_config = DreamServiceConfig {
            enabled: config.memory.memory_system.layer6.enabled,
            check_interval_secs: config.memory.memory_system.layer6.check_interval_secs,
            time_gate_threshold_hours: config.memory.memory_system.layer6.time_gate_threshold_hours
                as f64,
            session_gate_threshold: config.memory.memory_system.layer6.session_gate_threshold,
            timeout_secs: config.memory.memory_system.layer6.timeout_secs,
            provider_pool: Some(Arc::clone(&provider_pool)),
        };
        let dream_service = DreamService::new(dream_config, paths.clone());
        let dream_shutdown_rx = shutdown_tx.subscribe();
        let _dream_handle = tokio::spawn(async move {
            dream_service.run_loop(dream_shutdown_rx).await;
        });
        info!("[dream] Dream service started for cross-session knowledge consolidation");

        // 共享当前输入行和光标位置状态，用于事件处理器在打印后台结果/进度时
        // 先清除输入行和建议，打印完毕后重新渲染提示（含光标位置）
        let current_input: Arc<std::sync::Mutex<(String, usize)>> =
            Arc::new(std::sync::Mutex::new((String::new(), 0)));

        // Spawn event handler for streaming token output
        let event_handler_handle = {
            let current_input = current_input.clone();
            tokio::spawn(async move {
                use std::io::Write;
                let mut stdout = std::io::stdout();
                // Track whether streaming tokens were emitted for the current response.
                // If true, message_done should NOT reprint the content (avoid duplicate).
                // If false (non-streaming path like skill loops), message_done prints content.
                let mut emitted_text_delta = false;
                let mut emitted_thinking = false;
                loop {
                    match event_rx.recv().await {
                        Ok(event_str) => {
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&event_str)
                            {
                                let event_type =
                                    event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                match event_type {
                                    "token" => {
                                        // Streaming text token - print immediately
                                        if let Some(delta) =
                                            event.get("delta").and_then(|v| v.as_str())
                                        {
                                            // 在 thinking 之后、第一个 token 之前插入换行
                                            if emitted_thinking {
                                                println!();
                                                emitted_thinking = false;
                                            }
                                            emitted_text_delta = true;
                                            print!("{}", delta);
                                            let _ = stdout.flush();
                                        }
                                    }
                                    "thinking" => {
                                        // Thinking/reasoning content
                                        if let Some(content) =
                                            event.get("content").and_then(|v| v.as_str())
                                        {
                                            emitted_thinking = true;
                                            print!("{}", content);
                                            let _ = stdout.flush();
                                        }
                                    }
                                    "tool_call_start" => {
                                        // Tool call started
                                        if let Some(tool) =
                                            event.get("tool").and_then(|v| v.as_str())
                                        {
                                            let summary = event
                                                .get("summary")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            // 如果有 agent_type，说明是子agent的工具调用
                                            let agent_type = event
                                                .get("agent_type")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let task_id_short = event
                                                .get("task_id")
                                                .and_then(|v| v.as_str())
                                                .map(|s| short_task_id(s, 4))
                                                .unwrap_or_default();
                                            // 清除当前输入行，避免与提示重叠
                                            clear_prompt_line(&current_input, &mut stdout);
                                            if agent_type.is_empty() {
                                                if summary.is_empty() {
                                                    tracing::info!(
                                                        tool = tool,
                                                        "main agent tool call start"
                                                    );
                                                    eprintln!("\n🔧 {}", tool);
                                                } else {
                                                    tracing::info!(
                                                        tool = tool,
                                                        summary = summary,
                                                        "main agent tool call start"
                                                    );
                                                    eprintln!("\n🔧 {}({})", tool, summary);
                                                }
                                            } else if task_id_short.is_empty() {
                                                if summary.is_empty() {
                                                    tracing::info!(
                                                        agent_type = agent_type,
                                                        tool = tool,
                                                        "sub-agent tool call start"
                                                    );
                                                    eprintln!("  🔧 [{}] {}", agent_type, tool);
                                                } else {
                                                    tracing::info!(
                                                        agent_type = agent_type,
                                                        tool = tool,
                                                        summary = summary,
                                                        "sub-agent tool call start"
                                                    );
                                                    eprintln!(
                                                        "  🔧 [{}] {}({})",
                                                        agent_type, tool, summary
                                                    );
                                                }
                                            } else {
                                                if summary.is_empty() {
                                                    tracing::info!(agent_type = agent_type, task_id = %task_id_short, tool = tool, "sub-agent tool call start");
                                                    eprintln!(
                                                        "  🔧 [{}:{}] {}",
                                                        agent_type, task_id_short, tool
                                                    );
                                                } else {
                                                    tracing::info!(agent_type = agent_type, task_id = %task_id_short, tool = tool, summary = summary, "sub-agent tool call start");
                                                    eprintln!(
                                                        "  🔧 [{}:{}] {}({})",
                                                        agent_type, task_id_short, tool, summary
                                                    );
                                                }
                                            }
                                            // 恢复提示行
                                            restore_prompt_line(&current_input, &mut stdout);
                                        }
                                    }
                                    "tool_call_end" => {
                                        // 子 agent 工具调用完成
                                        let agent_type = event
                                            .get("agent_type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let task_id_short = event
                                            .get("task_id")
                                            .and_then(|v| v.as_str())
                                            .map(|s| short_task_id(s, 4))
                                            .unwrap_or_default();
                                        let tool = event
                                            .get("tool")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let success = event
                                            .get("success")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(true);
                                        if !agent_type.is_empty() && !tool.is_empty() && !success {
                                            clear_prompt_line(&current_input, &mut stdout);
                                            if task_id_short.is_empty() {
                                                tracing::info!(
                                                    agent_type = agent_type,
                                                    tool = tool,
                                                    "sub-agent tool call failed"
                                                );
                                                eprintln!("  ✗ [{}] {} failed", agent_type, tool);
                                            } else {
                                                tracing::info!(agent_type = agent_type, task_id = %task_id_short, tool = tool, "sub-agent tool call failed");
                                                eprintln!(
                                                    "  ✗ [{}:{}] {} failed",
                                                    agent_type, task_id_short, tool
                                                );
                                            }
                                            restore_prompt_line(&current_input, &mut stdout);
                                        }
                                    }
                                    "message_done" => {
                                        // Message complete
                                        // 检查是否是子agent汇总结果
                                        let is_summary = event
                                            .get("summary_for_subagents")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false);
                                        if is_summary {
                                            // 主agent汇总子agent结果，直接打印
                                            if let Some(content) =
                                                event.get("content").and_then(|v| v.as_str())
                                            {
                                                if !content.is_empty() {
                                                    clear_prompt_line(&current_input, &mut stdout);
                                                    tracing::info!("sub-agent summary delivered");
                                                    eprintln!("\n📋 **子agent结果汇总**");
                                                    println!("{}", content);
                                                    eprintln!("--- end ---");
                                                    println!();
                                                    restore_prompt_line(
                                                        &current_input,
                                                        &mut stdout,
                                                    );
                                                    // 标记已输出，防止后续 message_done 重复打印
                                                    emitted_text_delta = true;
                                                }
                                            }
                                        } else {
                                            // For subagent results (background_delivery=true), print the content
                                            // since it wasn't streamed via token events.
                                            // For normal streaming responses, just print a newline.
                                            let is_background = event
                                                .get("background_delivery")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false);
                                            if is_background {
                                                if let Some(content) =
                                                    event.get("content").and_then(|v| v.as_str())
                                                {
                                                    if !content.is_empty() {
                                                        // 获取 agent_type 用于标识来源
                                                        let agent_type = event
                                                            .get("agent_type")
                                                            .and_then(|v| v.as_str())
                                                            .unwrap_or("agent");
                                                        let task_id_short = event
                                                            .get("task_id")
                                                            .and_then(|v| v.as_str())
                                                            .map(|s| short_task_id(s, 8))
                                                            .unwrap_or_default();
                                                        // 清除当前输入行，打印结果，然后恢复提示
                                                        clear_prompt_line(
                                                            &current_input,
                                                            &mut stdout,
                                                        );
                                                        tracing::info!(
                                                            agent_type = agent_type,
                                                            task_id = %task_id_short,
                                                            "sub-agent background result delivered"
                                                        );
                                                        eprintln!(
                                                            "\n--- {} agent [{}] result ---",
                                                            agent_type, task_id_short
                                                        );
                                                        println!("{}", content);
                                                        eprintln!("--- end ---");
                                                        println!();
                                                        restore_prompt_line(
                                                            &current_input,
                                                            &mut stdout,
                                                        );
                                                    }
                                                }
                                            } else {
                                                // Non-streaming response: print content if not already emitted via tokens
                                                if !emitted_text_delta {
                                                    if let Some(content) = event
                                                        .get("content")
                                                        .and_then(|v| v.as_str())
                                                    {
                                                        if !content.is_empty() {
                                                            println!("\n{}", content);
                                                        }
                                                    }
                                                }
                                                println!();
                                                emitted_text_delta = false;
                                                emitted_thinking = false;
                                            }
                                        }
                                    }
                                    "agent_progress" => {
                                        // 子 agent 进度事件
                                        let agent_type = event
                                            .get("agent_type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("agent");
                                        let task_id_short = event
                                            .get("task_id")
                                            .and_then(|v| v.as_str())
                                            .map(|s| short_task_id(s, 4))
                                            .unwrap_or_default();
                                        let stage = event
                                            .get("stage")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let percent = event
                                            .get("percent")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0);
                                        if !stage.is_empty() {
                                            clear_prompt_line(&current_input, &mut stdout);
                                            let label = if task_id_short.is_empty() {
                                                agent_type.to_string()
                                            } else {
                                                format!("{}:{}", agent_type, task_id_short)
                                            };
                                            // 同时输出到 tracing（写入日志文件）和 eprintln（终端显示）
                                            tracing::info!(
                                                label = %label,
                                                stage = %stage,
                                                percent = percent,
                                                "sub-agent progress"
                                            );
                                            if percent > 0 {
                                                eprintln!("  [{}] {} ({}%)", label, stage, percent);
                                            } else {
                                                eprintln!("  [{}] {}", label, stage);
                                            }
                                            restore_prompt_line(&current_input, &mut stdout);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // Receiver 落后于发送者，跳过 n 条消息，继续接收
                            tracing::warn!(skipped = n, "Event receiver lagged, skipping messages");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            // 所有 sender 已关闭，退出循环
                            break;
                        }
                    }
                }
            })
        };

        // 启动核心进化 worker 后台任务
        let evo_shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            evo_worker_arc.run_loop(evo_shutdown_rx).await;
        });
        let skill_evo_shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            skill_evo_worker_arc.run_loop(skill_evo_shutdown_rx).await;
        });

        // Spawn runtime loop
        let runtime_handle = tokio::spawn(async move {
            runtime.run_loop(inbound_rx, None).await;
        });

        // Split outbound: channel messages go to ChannelManager, CLI messages go to printer
        // Note: "cli" messages are already printed via streaming events (token + message_done),
        // so we skip them here to avoid duplicate output.
        let (printer_tx, mut printer_rx) = mpsc::channel(100);
        let outbound_dispatch_handle = tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                match msg.channel.as_str() {
                    "cli" => {
                        // Print content if present (skill loops use non-streaming calls).
                        // skip_ws_echo: 对于ws渠道，流式token已通过event_tx输出，避免重复
                        // 对于CLI渠道，skip_ws_echo=true表示流式token已打印，跳过outbound重复输出
                        if !msg.content.is_empty() && !msg.skip_ws_echo {
                            let _ = printer_tx.send(msg).await;
                        }
                    }
                    "cron" => {
                        let _ = printer_tx.send(msg).await;
                    }
                    _ => {
                        // Dispatch to external channel (Telegram/Slack/Discord/etc.)
                        if let Err(e) = channel_manager.dispatch_outbound_msg(&msg).await {
                            tracing::error!(error = %e, channel = %msg.channel, "Failed to dispatch outbound message");
                        }
                    }
                }
            }
        });

        // Spawn outbound printer — prints responses from CLI and cron jobs
        let printer_handle = {
            let current_input = current_input.clone();
            tokio::spawn(async move {
                let mut stdout = std::io::stdout();
                while let Some(msg) = printer_rx.recv().await {
                    clear_prompt_line(&current_input, &mut stdout);
                    if msg.channel == "cron" {
                        println!("\n[cron] {}", msg.content);
                    } else {
                        println!("\n{}", msg.content);
                    }
                    println!();
                    restore_prompt_line(&current_input, &mut stdout);
                }
            })
        };

        // Channel for the confirm handler to send a oneshot::Sender to the stdin thread,
        // so the stdin thread can route the next line of input as a confirmation response.
        let (confirm_answer_tx, confirm_answer_rx) =
            std::sync::mpsc::channel::<tokio::sync::oneshot::Sender<bool>>();

        // Spawn confirmation handler — receives ConfirmRequest from runtime,
        // prints the prompt, and delegates the actual stdin read to the stdin thread.
        let confirm_handle = tokio::spawn(async move {
            while let Some(request) = confirm_rx.recv().await {
                // Print confirmation prompt
                eprintln!();
                eprintln!("⚠️  Security confirmation: tool `{}` requests access to paths outside workspace:", request.tool_name);
                for p in &request.paths {
                    eprintln!("   📁 {}", p);
                }
                eprint!("Allow? (y/n): ");
                let _ = std::io::Write::flush(&mut std::io::stderr());

                // Send the response channel to the stdin thread so it can answer
                if confirm_answer_tx.send(request.response_tx).is_err() {
                    break;
                }
            }
        });

        // Single stdin reader thread — routes input to either message or confirmation.
        // The confirm handler prints the prompt and sends a oneshot::Sender here.
        // After each read_line, we check if a confirmation is pending and route accordingly.
        // Clone paths for the stdin thread (needed for skill management commands)
        let stdin_paths = paths.clone();

        let stdin_tx = inbound_tx.clone();
        let session_clone = session.clone();
        let stdin_task_manager = task_manager.clone();
        let stdin_checkpoint_manager = CheckpointManager::new(&paths.workspace());

        // 创建会话清除标记（用于 /clear 命令）
        let session_clear_flag = Arc::new(AtomicBool::new(false));
        let session_clear_flag_clone = session_clear_flag.clone();
        let response_cache_for_stdin = response_cache.clone();
        let stdin_current_input = current_input.clone();

        let stdin_handle = tokio::task::spawn_blocking(move || {
            let mut stdout = std::io::stdout();
            // Create a small tokio runtime for blocking task manager queries
            let local_rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create local runtime for stdin");

            loop {
                // Note: prompt is printed inside read_line_with_command_picker
                // to avoid double printing after raw mode is enabled

                // Read input character by character to detect "/" immediately
                let input = read_line_with_command_picker(
                    &stdin_paths,
                    &mut stdout,
                    &session_clone,
                    &stdin_tx,
                    &stdin_current_input,
                );

                // Check if a confirmation request arrived
                if let Ok(response_tx) = confirm_answer_rx.try_recv() {
                    let answer = input.trim().to_lowercase();
                    let allowed = answer == "y" || answer == "yes";
                    if allowed {
                        eprintln!("✅ Access granted");
                    } else {
                        eprintln!("❌ Access denied");
                    }
                    eprintln!();
                    let _ = response_tx.send(allowed);
                    continue;
                }

                let input = input.trim().to_string();
                if input.is_empty() {
                    continue;
                }

                // 使用统一的斜杠命令处理器
                if input.starts_with('/') {
                    // 构造命令上下文
                    let ctx = CommandContext::for_cli(
                        stdin_paths.clone(),
                        stdin_task_manager.clone(),
                        stdin_checkpoint_manager.clone(),
                        session_clone
                            .split(':')
                            .nth(1)
                            .unwrap_or("default")
                            .to_string(),
                    )
                    .with_clear_callback(Arc::new({
                        let flag = session_clear_flag_clone.clone();
                        move || {
                            flag.store(true, Ordering::SeqCst);
                            true
                        }
                    }));

                    // 同步执行命令处理器
                    let result = local_rt.block_on(SLASH_COMMAND_HANDLER.try_handle(&input, &ctx));

                    match result {
                        CommandResult::Handled(response) => {
                            print!("{}", response.content);
                            continue;
                        }
                        CommandResult::ExitRequested => {
                            println!("退出交互模式...");
                            break;
                        }
                        CommandResult::NotACommand => {
                            // 不是斜杠命令，继续正常消息处理流程
                        }
                        CommandResult::PermissionDenied(msg) => {
                            eprintln!("权限不足: {}", msg);
                            continue;
                        }
                        CommandResult::Error(e) => {
                            eprintln!("命令执行错误: {}", e);
                            continue;
                        }
                        CommandResult::ForwardToRuntime {
                            transformed_content,
                            original_command,
                        } => {
                            // 命令需要转发给 AgentRuntime（如 /learn, /cancel-task, /resume）
                            tracing::info!(
                                command = %original_command,
                                "Forwarding command to AgentRuntime"
                            );
                            let inbound = InboundMessage {
                                channel: "cli".to_string(),
                                account_id: None,
                                sender_id: "user".to_string(),
                                chat_id: session_clone
                                    .split(':')
                                    .nth(1)
                                    .unwrap_or("default")
                                    .to_string(),
                                content: transformed_content,
                                media: vec![],
                                // 标记来源为斜杠命令，runtime 据此验证授权
                                metadata: serde_json::json!({
                                    "source": "slash_command",
                                    "original_command": original_command
                                }),
                                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            };
                            if stdin_tx.blocking_send(inbound).is_err() {
                                break;
                            }
                            continue;
                        }
                    }
                }

                // 检查会话清除标记（由 /clear 命令设置）
                if session_clear_flag_clone.load(Ordering::SeqCst) {
                    // 标记已处理，重置
                    session_clear_flag_clone.store(false, Ordering::SeqCst);
                    // 清除内存中的 ResponseCache
                    response_cache_for_stdin.clear_session(&session_clone);
                    tracing::info!(session = %session_clone, "[/clear] ResponseCache cleared");
                }

                // Extract image paths from input for multimodal support
                let (text, media) = extract_media_from_input(&input);
                if !media.is_empty() {
                    eprintln!("  📎 Detected {} image(s)", media.len());
                }
                let inbound = InboundMessage {
                    channel: "cli".to_string(),
                    account_id: None,
                    sender_id: "user".to_string(),
                    chat_id: session_clone
                        .split(':')
                        .nth(1)
                        .unwrap_or("default")
                        .to_string(),
                    content: if media.is_empty() { input } else { text },
                    media,
                    metadata: serde_json::Value::Null,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                };

                if stdin_tx.blocking_send(inbound).is_err() {
                    break;
                }
            }
        });

        // Wait for stdin to finish (user typed /quit or Ctrl+D)
        let _ = stdin_handle.await;

        info!("Shutting down agent...");

        // Stop cleanup loop
        cleanup_handle.abort();

        let _ = shutdown_tx.send(());

        // Drop inbound_tx to close the channel and stop runtime
        drop(inbound_tx);

        let mut handles: Vec<tokio::task::JoinHandle<()>> = vec![
            runtime_handle,
            cron_handle,
            printer_handle,
            confirm_handle,
            outbound_dispatch_handle,
            event_handler_handle,
        ];
        handles.extend(channel_handles);

        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            futures::future::join_all(handles),
        )
        .await;
    }

    Ok(())
}

/// 计算字符串在终端中的总显示宽度（列数）。
/// 使用 unicode-width 库的字符串级宽度计算，正确处理多码点序列。
fn str_display_width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// 计算字符串中的 grapheme cluster 数量。
fn grapheme_count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// 将 grapheme 索引转换为字符串中的字节索引。
fn grapheme_to_byte_index(s: &str, grapheme_idx: usize) -> usize {
    s.grapheme_indices(true)
        .nth(grapheme_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

/// Read a line of input with real-time command picker support.
/// When user types "/", immediately show command suggestions below the input line.
/// Supports backspace to delete, left/right cursor movement, and escape to cancel picker.
fn read_line_with_command_picker(
    paths: &Paths,
    stdout: &mut std::io::Stdout,
    _session: &str,
    _stdin_tx: &mpsc::Sender<InboundMessage>,
    current_input: &std::sync::Mutex<(String, usize)>,
) -> String {
    let mut input = String::new();
    // 同步共享输入状态
    if let Ok(mut shared) = current_input.lock() {
        shared.0.clear();
        shared.1 = 0;
    }
    let all_items = collect_command_items(paths);
    let mut selected_index: usize = 0;
    let mut showing_picker = false;
    let mut visible_count: usize = 0;
    let mut visible_limit: usize = 16; // Initial items to show
    let mut prev_visible_limit: usize = 0; // Track previous limit for proper clearing
    let mut command_start_pos: Option<usize> = None; // Position of '/' for command
    let mut cursor_pos: usize = 0; // 光标在输入字符串中的 grapheme 索引
    const LOAD_MORE_COUNT: usize = 10; // Items to load when scrolling to end

    // Enable raw mode for character-by-character input
    // This disables line buffering and echo on both Unix and Windows
    // If raw mode fails, we fall back to standard input mode
    if let Err(e) = terminal::enable_raw_mode() {
        // Raw mode failed - use fallback with std::io::stdin
        // This means we won't have command picker, but basic input will work
        eprintln!(
            "Warning: Failed to enable raw mode: {}. Using fallback input.",
            e
        );
        let _ = terminal::disable_raw_mode(); // Ensure clean state
        use std::io::{self, BufRead};
        let stdin = io::stdin();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_ok() {
            return line
                .trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_string();
        }
        return String::new();
    }

    // Initial prompt - use crossterm commands for proper terminal control
    let _ = execute!(
        stdout,
        Print("\r"),
        Clear(ClearType::CurrentLine),
        Print("> ")
    );

    loop {
        match event::read() {
            Ok(Event::Key(key)) => {
                // On Windows, we receive both Press and Release events.
                // Only process Press events to avoid double input.
                if key.kind == KeyEventKind::Release {
                    continue;
                }

                match key.code {
                    KeyCode::Char(c) => {
                        if c == 'c' && key.modifiers.contains(KeyModifiers::CONTROL) {
                            let _ = terminal::disable_raw_mode();
                            println!();
                            std::process::exit(0);
                        }

                        // 在光标位置插入字符
                        let byte_idx = grapheme_to_byte_index(&input, cursor_pos);
                        input.insert(byte_idx, c);
                        cursor_pos = grapheme_count(&input[..byte_idx + c.len_utf8()]);
                        if let Ok(mut shared) = current_input.lock() {
                            *shared = (input.clone(), cursor_pos);
                        }

                        // Check if we should show suggestions - detect '/' anywhere
                        if let Some((pos, query)) = extract_command_query(&input) {
                            if !showing_picker {
                                showing_picker = true;
                            }
                            command_start_pos = Some(pos);
                            selected_index = 0;
                            visible_limit = 16;
                            visible_count = render_suggestions(
                                &all_items,
                                query,
                                &input,
                                selected_index,
                                visible_limit,
                                prev_visible_limit,
                                stdout,
                                cursor_pos,
                            );
                            prev_visible_limit = visible_limit;
                        } else if showing_picker {
                            clear_suggestions(prev_visible_limit, &input, stdout, cursor_pos);
                            prev_visible_limit = 0;
                            showing_picker = false;
                            visible_count = 0;
                            command_start_pos = None;
                        } else {
                            render_input_line(&input, stdout, cursor_pos);
                        }

                        use std::io::Write;
                        let _ = stdout.flush();
                    }
                    KeyCode::Enter => {
                        // If showing picker, select current item
                        if showing_picker && visible_count > 0 {
                            let query = extract_command_query(&input).map(|(_, q)| q).unwrap_or("");
                            let filtered = filter_items(&all_items, query);

                            if let Some(item) = filtered.get(selected_index) {
                                // Clear suggestions first
                                clear_suggestions(prev_visible_limit, &input, stdout, cursor_pos);
                                prev_visible_limit = 0;
                                // Replace command part with selected item
                                if let Some(pos) = command_start_pos {
                                    input = format!("{} /{} ", &input[..pos], item.name);
                                } else {
                                    input = format!("/{} ", item.name);
                                }
                                cursor_pos = grapheme_count(&input);
                                if let Ok(mut shared) = current_input.lock() {
                                    *shared = (input.clone(), cursor_pos);
                                }
                                render_input_line(&input, stdout, cursor_pos);
                                showing_picker = false;
                                visible_count = 0;
                                command_start_pos = None;
                                continue;
                            }
                        }

                        // Submit the input
                        if showing_picker {
                            clear_suggestions(prev_visible_limit, &input, stdout, cursor_pos);
                        }
                        // 清除共享输入状态，提交后不再需要恢复提示
                        if let Ok(mut shared) = current_input.lock() {
                            shared.0.clear();
                            shared.1 = 0;
                        }
                        let _ = terminal::disable_raw_mode();
                        println!();
                        return input;
                    }
                    KeyCode::Tab => {
                        // Select current item in picker
                        if showing_picker && visible_count > 0 {
                            let query = extract_command_query(&input).map(|(_, q)| q).unwrap_or("");
                            let filtered = filter_items(&all_items, query);

                            if let Some(item) = filtered.get(selected_index) {
                                // Clear suggestions first
                                clear_suggestions(prev_visible_limit, &input, stdout, cursor_pos);
                                prev_visible_limit = 0;
                                // Replace command part with selected item
                                if let Some(pos) = command_start_pos {
                                    input = format!("{} /{} ", &input[..pos], item.name);
                                } else {
                                    input = format!("/{} ", item.name);
                                }
                                cursor_pos = grapheme_count(&input);
                                if let Ok(mut shared) = current_input.lock() {
                                    *shared = (input.clone(), cursor_pos);
                                }
                                render_input_line(&input, stdout, cursor_pos);
                                showing_picker = false;
                                visible_count = 0;
                                command_start_pos = None;
                            }
                        }
                    }
                    KeyCode::Up => {
                        if showing_picker && visible_count > 0 && selected_index > 0 {
                            selected_index -= 1;
                            let query = extract_command_query(&input).map(|(_, q)| q).unwrap_or("");
                            visible_count = render_suggestions(
                                &all_items,
                                query,
                                &input,
                                selected_index,
                                visible_limit,
                                prev_visible_limit,
                                stdout,
                                cursor_pos,
                            );
                            prev_visible_limit = visible_limit;
                        }
                    }
                    KeyCode::Down => {
                        if showing_picker && visible_count > 0 {
                            // visible_limit is how many we're showing, visible_count is total available
                            let displayed_count = visible_limit.min(visible_count);
                            let last_displayed_idx = displayed_count.saturating_sub(1);
                            let last_total_idx = visible_count.saturating_sub(1);

                            let query = extract_command_query(&input).map(|(_, q)| q).unwrap_or("");

                            // Check if we're at the last displayed item and there are more items to load
                            if selected_index == last_displayed_idx
                                && selected_index < last_total_idx
                            {
                                // Load more items
                                visible_limit += LOAD_MORE_COUNT;
                                selected_index += 1;
                                visible_count = render_suggestions(
                                    &all_items,
                                    query,
                                    &input,
                                    selected_index,
                                    visible_limit,
                                    prev_visible_limit,
                                    stdout,
                                    cursor_pos,
                                );
                                prev_visible_limit = visible_limit;
                            } else if selected_index < last_displayed_idx {
                                // Normal navigation within displayed items
                                selected_index += 1;
                                visible_count = render_suggestions(
                                    &all_items,
                                    query,
                                    &input,
                                    selected_index,
                                    visible_limit,
                                    prev_visible_limit,
                                    stdout,
                                    cursor_pos,
                                );
                                prev_visible_limit = visible_limit;
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        if cursor_pos > 0 {
                            cursor_pos -= 1;
                            let byte_idx = grapheme_to_byte_index(&input, cursor_pos);
                            let next_byte_idx = grapheme_to_byte_index(&input, cursor_pos + 1);
                            input.drain(byte_idx..next_byte_idx);
                            if let Ok(mut shared) = current_input.lock() {
                                *shared = (input.clone(), cursor_pos);
                            }

                            // 删除后重新检测命令模式，更新命令起始位置
                            if let Some((pos, query)) = extract_command_query(&input) {
                                command_start_pos = Some(pos);
                                showing_picker = true;
                                selected_index = 0;
                                visible_limit = 16;
                                visible_count = render_suggestions(
                                    &all_items,
                                    query,
                                    &input,
                                    selected_index,
                                    visible_limit,
                                    prev_visible_limit,
                                    stdout,
                                    cursor_pos,
                                );
                                prev_visible_limit = visible_limit;
                            } else {
                                if showing_picker && visible_count > 0 {
                                    clear_suggestions(
                                        prev_visible_limit,
                                        &input,
                                        stdout,
                                        cursor_pos,
                                    );
                                    prev_visible_limit = 0;
                                }
                                showing_picker = false;
                                visible_count = 0;
                                command_start_pos = None;
                                render_input_line(&input, stdout, cursor_pos);
                            }

                            use std::io::Write;
                            let _ = stdout.flush();
                        }
                    }
                    // Delete 键：删除光标后面的字符（向前删除）
                    KeyCode::Delete => {
                        let grapheme_len = grapheme_count(&input);
                        if cursor_pos < grapheme_len {
                            let byte_idx = grapheme_to_byte_index(&input, cursor_pos);
                            let next_byte_idx = grapheme_to_byte_index(&input, cursor_pos + 1);
                            input.drain(byte_idx..next_byte_idx);
                            if let Ok(mut shared) = current_input.lock() {
                                *shared = (input.clone(), cursor_pos);
                            }

                            // 删除后重新检测命令模式，更新命令起始位置
                            if let Some((pos, query)) = extract_command_query(&input) {
                                command_start_pos = Some(pos);
                                showing_picker = true;
                                selected_index = 0;
                                visible_limit = 16;
                                visible_count = render_suggestions(
                                    &all_items,
                                    query,
                                    &input,
                                    selected_index,
                                    visible_limit,
                                    prev_visible_limit,
                                    stdout,
                                    cursor_pos,
                                );
                                prev_visible_limit = visible_limit;
                            } else {
                                if showing_picker && visible_count > 0 {
                                    clear_suggestions(
                                        prev_visible_limit,
                                        &input,
                                        stdout,
                                        cursor_pos,
                                    );
                                    prev_visible_limit = 0;
                                }
                                showing_picker = false;
                                visible_count = 0;
                                command_start_pos = None;
                                render_input_line(&input, stdout, cursor_pos);
                            }

                            use std::io::Write;
                            let _ = stdout.flush();
                        }
                    }
                    KeyCode::Left => {
                        if cursor_pos > 0 {
                            cursor_pos -= 1;
                            if let Ok(mut shared) = current_input.lock() {
                                *shared = (input.clone(), cursor_pos);
                            }
                            if showing_picker {
                                let query =
                                    extract_command_query(&input).map(|(_, q)| q).unwrap_or("");
                                visible_count = render_suggestions(
                                    &all_items,
                                    query,
                                    &input,
                                    selected_index,
                                    visible_limit,
                                    prev_visible_limit,
                                    stdout,
                                    cursor_pos,
                                );
                                prev_visible_limit = visible_limit;
                            } else {
                                render_input_line(&input, stdout, cursor_pos);
                            }
                            use std::io::Write;
                            let _ = stdout.flush();
                        }
                    }
                    KeyCode::Right => {
                        let grapheme_len = grapheme_count(&input);
                        if cursor_pos < grapheme_len {
                            cursor_pos += 1;
                            if let Ok(mut shared) = current_input.lock() {
                                *shared = (input.clone(), cursor_pos);
                            }
                            if showing_picker {
                                let query =
                                    extract_command_query(&input).map(|(_, q)| q).unwrap_or("");
                                visible_count = render_suggestions(
                                    &all_items,
                                    query,
                                    &input,
                                    selected_index,
                                    visible_limit,
                                    prev_visible_limit,
                                    stdout,
                                    cursor_pos,
                                );
                                prev_visible_limit = visible_limit;
                            } else {
                                render_input_line(&input, stdout, cursor_pos);
                            }
                            use std::io::Write;
                            let _ = stdout.flush();
                        }
                    }
                    KeyCode::Home => {
                        if cursor_pos > 0 {
                            cursor_pos = 0;
                            if let Ok(mut shared) = current_input.lock() {
                                *shared = (input.clone(), cursor_pos);
                            }
                            if showing_picker {
                                let query =
                                    extract_command_query(&input).map(|(_, q)| q).unwrap_or("");
                                visible_count = render_suggestions(
                                    &all_items,
                                    query,
                                    &input,
                                    selected_index,
                                    visible_limit,
                                    prev_visible_limit,
                                    stdout,
                                    cursor_pos,
                                );
                                prev_visible_limit = visible_limit;
                            } else {
                                render_input_line(&input, stdout, cursor_pos);
                            }
                            use std::io::Write;
                            let _ = stdout.flush();
                        }
                    }
                    KeyCode::End => {
                        let grapheme_len = grapheme_count(&input);
                        if cursor_pos < grapheme_len {
                            cursor_pos = grapheme_len;
                            if let Ok(mut shared) = current_input.lock() {
                                *shared = (input.clone(), cursor_pos);
                            }
                            if showing_picker {
                                let query =
                                    extract_command_query(&input).map(|(_, q)| q).unwrap_or("");
                                visible_count = render_suggestions(
                                    &all_items,
                                    query,
                                    &input,
                                    selected_index,
                                    visible_limit,
                                    prev_visible_limit,
                                    stdout,
                                    cursor_pos,
                                );
                                prev_visible_limit = visible_limit;
                            } else {
                                render_input_line(&input, stdout, cursor_pos);
                            }
                            use std::io::Write;
                            let _ = stdout.flush();
                        }
                    }
                    KeyCode::Esc => {
                        if showing_picker {
                            clear_suggestions(prev_visible_limit, &input, stdout, cursor_pos);
                            prev_visible_limit = 0;
                            showing_picker = false;
                            visible_count = 0;
                            command_start_pos = None;
                            render_input_line(&input, stdout, cursor_pos);
                            use std::io::Write;
                            let _ = stdout.flush();
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Resize(_, _)) => {
                // Terminal resize - re-render if showing picker
                if showing_picker {
                    let query = extract_command_query(&input).map(|(_, q)| q).unwrap_or("");
                    visible_count = render_suggestions(
                        &all_items,
                        query,
                        &input,
                        selected_index,
                        visible_limit,
                        prev_visible_limit,
                        stdout,
                        cursor_pos,
                    );
                    prev_visible_limit = visible_limit;
                } else {
                    render_input_line(&input, stdout, cursor_pos);
                }
            }
            Ok(_) => {
                // Ignore other events
            }
            Err(_) => {
                let _ = terminal::disable_raw_mode();
                return input;
            }
        }
    }
}

/// Clear the current input line (including any suggestions), preparing for
/// 从 task_id 字符串中提取短且有意义的标识符。
///
/// Task ID 格式为 "task-{uuid_prefix}"（如 "task-bec116a0"）。
/// 直接取前N个字符会得到无意义的 "task"。
/// 此函数先剥离 "task-" 前缀，再从 UUID 部分取字符。
///
/// # Examples
/// - `short_task_id("task-bec116a0", 4)` → `"bec1"`
/// - `short_task_id("task-816ca144", 4)` → `"816c"`
/// - `short_task_id("some-other-id", 4)` → `"some"` (无前缀匹配，回退)
/// - `short_task_id("", 4)` → `""`
fn short_task_id(task_id: &str, max_chars: usize) -> String {
    if task_id.is_empty() {
        return String::new();
    }
    // 剥离 "task-" 前缀，取有意义的 UUID 部分
    let meaningful = if let Some(rest) = task_id.strip_prefix("task-") {
        rest
    } else {
        task_id
    };
    meaningful.chars().take(max_chars).collect()
}

/// an interrupting output (e.g. background agent result, progress).
/// After the caller prints its content, it should call `restore_prompt_line`.
fn clear_prompt_line(
    _current_input: &std::sync::Mutex<(String, usize)>,
    stdout: &mut std::io::Stdout,
) {
    use std::io::Write;
    // Clear current line and move to start
    let _ = execute!(stdout, Print("\r"), Clear(ClearType::CurrentLine));
    let _ = stdout.flush();
    // Note: we don't know how many suggestion lines are visible,
    // but since we're in raw mode the cursor is on the input line,
    // so clearing CurrentLine is sufficient. Any suggestions below
    // will be overwritten when we restore the prompt.
}

/// 后台输出后恢复提示行。
/// 从共享状态中读取输入内容和光标位置，重新渲染并定位光标。
fn restore_prompt_line(
    current_input: &std::sync::Mutex<(String, usize)>,
    stdout: &mut std::io::Stdout,
) {
    let guard = current_input.lock().unwrap_or_else(|e| e.into_inner());
    let (input, cursor_pos) = (guard.0.clone(), guard.1);
    drop(guard);
    render_input_line(&input, stdout, cursor_pos);
    use std::io::Write;
    let _ = stdout.flush();
}

/// 使用 crossterm 渲染输入行并定位光标。
/// `cursor_pos` 是光标在 `input` 中的 grapheme 索引（从 0 开始）。
fn render_input_line(input: &str, stdout: &mut std::io::Stdout, cursor_pos: usize) {
    use std::io::Write;
    let _ = execute!(
        stdout,
        Print("\r"),
        Clear(ClearType::CurrentLine),
        Print(format!("> {}", input))
    );
    // Move cursor left by the display width of characters after cursor_pos
    let byte_idx = grapheme_to_byte_index(input, cursor_pos);
    let after_width = str_display_width(&input[byte_idx..]);
    if after_width > 0 {
        let _ = execute!(stdout, cursor::MoveLeft(after_width as u16));
    }
    let _ = stdout.flush();
}

/// Extract command query from input - finds the last '/' and returns the text after it
/// Only triggers if '/' is at the start or preceded by a space
/// Returns (position of '/', query string) if found and no space after '/'
fn extract_command_query(input: &str) -> Option<(usize, &str)> {
    // Find the last '/' in input
    if let Some(slash_pos) = input.rfind('/') {
        // Check if '/' is at the start or preceded by a space
        let is_at_start = slash_pos == 0;
        // Check if the part before '/' ends with a space (or is empty for start)
        let before_slash = &input[..slash_pos];
        let is_after_space = before_slash.ends_with(' ');

        if !is_at_start && !is_after_space {
            return None;
        }

        let after_slash = &input[slash_pos + 1..];
        // Check if there's no space in the command part (means still typing command)
        if !after_slash.contains(' ') {
            Some((slash_pos, after_slash))
        } else {
            None
        }
    } else {
        None
    }
}

/// Filter items based on query - returns all matching items sorted by relevance
fn filter_items<'a>(items: &'a [CommandItem], query: &str) -> Vec<&'a CommandItem> {
    if query.is_empty() {
        items.iter().collect()
    } else {
        let q = query.to_lowercase();
        // Score each item: name starts with query = 3, name contains query = 2, description contains query = 1
        let mut scored: Vec<(usize, &CommandItem)> = items
            .iter()
            .filter_map(|item| {
                let name_lower = item.name.to_lowercase();
                let desc_lower = item.description.to_lowercase();
                let score = if name_lower.starts_with(&q) {
                    3
                } else if name_lower.contains(&q) {
                    2
                } else if desc_lower.contains(&q) {
                    1
                } else {
                    0
                };
                if score > 0 {
                    Some((score, item))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score first (higher is better), then by name
        scored.sort_by(|a, b| {
            if b.0 != a.0 {
                b.0.cmp(&a.0)
            } else {
                a.1.name.cmp(&b.1.name)
            }
        });

        scored.into_iter().map(|(_, item)| item).collect()
    }
}

/// Render suggestions below the input line
/// Returns the total number of filtered items (not just displayed)
fn render_suggestions(
    all_items: &[CommandItem],
    query: &str,
    input: &str,
    selected: usize,
    visible_limit: usize,
    prev_lines_to_clear: usize,
    stdout: &mut std::io::Stdout,
    cursor_pos: usize,
) -> usize {
    let filtered = filter_items(all_items, query);
    let total_count = filtered.len();
    let display_count = filtered.len().min(visible_limit);
    let has_more = total_count > visible_limit;

    // First, clear all previously displayed lines plus potential new lines
    // Use the maximum of prev_lines_to_clear and current visible_limit
    let lines_to_clear = prev_lines_to_clear.max(visible_limit) + 1; // +1 for "show more" line
    let _ = execute!(stdout, cursor::SavePosition);
    for _ in 0..lines_to_clear {
        let _ = execute!(stdout, Print("\r\n"), Clear(ClearType::CurrentLine));
    }
    let _ = execute!(stdout, cursor::RestorePosition);

    if display_count == 0 {
        // 无匹配建议时，渲染输入行并正确定位光标
        render_input_line(input, stdout, cursor_pos);
        return 0;
    }

    // Calculate max name width for alignment
    let max_name_width = filtered
        .iter()
        .take(display_count)
        .map(|item| item.name.chars().count())
        .max()
        .unwrap_or(0);

    // Now print the suggestions - move down one line at a time
    for (i, item) in filtered.iter().take(display_count).enumerate() {
        let is_selected = i == selected;
        let icon = if item.kind == "tool" { "🔧" } else { "✨" };
        let kind_label = if item.kind == "tool" { "tool" } else { "skill" };
        let desc: String = item.description.chars().take(25).collect();

        // Pad name to align descriptions
        let name_width = item.name.chars().count();
        let padding = " ".repeat(max_name_width.saturating_sub(name_width));

        // Move to next line, clear it, print content
        let _ = execute!(stdout, Print("\r\n"), Clear(ClearType::CurrentLine));

        if is_selected {
            // Selected item with reverse video and bold
            let _ = execute!(
                stdout,
                Print(format!(
                    "\x1b[7m\x1b[1m {} {}{} \x1b[0m\x1b[90m[{}]\x1b[0m \x1b[2m{}\x1b[0m",
                    icon, item.name, padding, kind_label, desc
                ))
            );
        } else {
            let _ = execute!(
                stdout,
                Print(format!(
                    "   {} {}{}  \x1b[90m[{}]\x1b[0m \x1b[2m{}\x1b[0m",
                    icon, item.name, padding, kind_label, desc
                ))
            );
        }
    }

    // Show "show more" indicator if there are more items
    let mut extra_lines = 0;
    if has_more {
        let remaining = total_count - visible_limit;
        let _ = execute!(stdout, Print("\r\n"), Clear(ClearType::CurrentLine));
        let _ = execute!(
            stdout,
            Print(format!(
                "\x1b[90m   ↓ show more ({} remaining)\x1b[0m",
                remaining
            ))
        );
        extra_lines = 1;
    }

    // Move cursor back up to input line
    for _ in 0..(display_count + extra_lines) {
        let _ = execute!(stdout, cursor::MoveUp(1));
    }

    // Render input line
    let _ = execute!(
        stdout,
        Print("\r"),
        Clear(ClearType::CurrentLine),
        Print(format!("> {}", input))
    );
    // Position cursor correctly
    let byte_idx = grapheme_to_byte_index(input, cursor_pos);
    let after_width = str_display_width(&input[byte_idx..]);
    if after_width > 0 {
        let _ = execute!(stdout, cursor::MoveLeft(after_width as u16));
    }

    // Flush to ensure output is immediately visible
    use std::io::Write;
    let _ = stdout.flush();

    total_count
}

/// Clear the suggestion list
fn clear_suggestions(
    visible_limit: usize,
    input: &str,
    stdout: &mut std::io::Stdout,
    cursor_pos: usize,
) {
    // Save position, clear all suggestion lines (+1 for potential "show more" line), restore position
    let lines_to_clear = visible_limit + 1;
    let _ = execute!(stdout, cursor::SavePosition);
    for _ in 0..lines_to_clear {
        let _ = execute!(stdout, Print("\r\n"), Clear(ClearType::CurrentLine));
    }
    let _ = execute!(stdout, cursor::RestorePosition);

    // Render input line
    let _ = execute!(
        stdout,
        Print("\r"),
        Clear(ClearType::CurrentLine),
        Print(format!("> {}", input))
    );
    // Position cursor correctly
    let byte_idx = grapheme_to_byte_index(input, cursor_pos);
    let after_width = str_display_width(&input[byte_idx..]);
    if after_width > 0 {
        let _ = execute!(stdout, cursor::MoveLeft(after_width as u16));
    }

    // Flush to ensure output is immediately visible
    use std::io::Write;
    let _ = stdout.flush();
}

/// 判断目录是否包含 skill 标识文件
fn is_skill_dir(path: &std::path::Path) -> bool {
    path.join("SKILL.md").exists()
        || path.join("meta.yaml").exists()
        || path.join("meta.json").exists()
        || path.join("SKILL.rhai").exists()
        || path.join("SKILL.py").exists()
}

/// 排除的目录名
const SKILL_EXCLUDED_DIRS: &[&str] = &[".git", ".github", ".hub", "__pycache__", "node_modules"];

/// Scan a directory for skill subdirectories and collect (name, description) pairs.
/// Supports skill packs (manifest.json) and category directories with recursive scanning.
fn scan_skill_dirs(dir: &std::path::Path) -> Vec<(String, String)> {
    let mut skills = Vec::new();
    if !dir.is_dir() {
        return skills;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }

            let dir_name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if SKILL_EXCLUDED_DIRS.contains(&dir_name.as_str()) {
                continue;
            }

            // 目录本身是 skill：优先加载自身
            if is_skill_dir(&p) {
                let name = dir_name;
                let desc = read_skill_description_from_dir(&p);
                skills.push((name, desc));
                // 若同时是 skill 包（含 manifest.json），也递归扫描子目录
                if p.join("manifest.json").exists() {
                    skills.extend(scan_skill_dirs(&p));
                }
                continue;
            }

            // 非 skill 目录但含 manifest.json：作为 skill 包递归扫描子目录
            if p.join("manifest.json").exists() {
                skills.extend(scan_skill_dirs(&p));
                continue;
            }

            // 普通 category 目录：递归扫描其子目录寻找 skill
            skills.extend(scan_skill_dirs(&p));
        }
    }
    skills.sort_by(|a, b| a.0.cmp(&b.0));
    skills
}

/// 从技能目录读取描述（SKILL.md frontmatter / meta.yaml / meta.json）
fn read_skill_description_from_dir(path: &std::path::Path) -> String {
    // 1. 尝试 meta.yaml
    let meta_yaml = path.join("meta.yaml");
    if meta_yaml.exists() {
        if let Ok(content) = std::fs::read_to_string(&meta_yaml) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("description:") {
                    let val = trimmed.trim_start_matches("description:").trim();
                    let val = val.trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return val.to_string();
                    }
                }
            }
        }
    }

    // 2. 尝试 meta.json
    let meta_json = path.join("meta.json");
    if meta_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&meta_json) {
            if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(desc) = meta.get("description").and_then(|v| v.as_str()) {
                    if !desc.is_empty() {
                        return desc.to_string();
                    }
                }
            }
        }
    }

    // 3. 尝试 SKILL.md frontmatter
    let skill_md = path.join("SKILL.md");
    if let Ok(content) = std::fs::read_to_string(&skill_md) {
        // Extract frontmatter description
        let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
        let content_normalized = content.replace("\r\n", "\n");
        let trimmed = content_normalized.trim_start();
        if let Some(rest) = trimmed.strip_prefix("---") {
            if let Some(end_idx) = rest.find("\n---") {
                let frontmatter = rest[..end_idx].trim();
                if let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter) {
                    if let Some(desc) = yaml.get("description").and_then(|v| v.as_str()) {
                        if !desc.is_empty() {
                            return desc.trim().to_string();
                        }
                    }
                }
            }
        }
        // Fallback: first heading
        for line in content.lines() {
            let line = line.trim();
            if let Some(heading) = line.strip_prefix("# ") {
                return heading.to_string();
            }
        }
    }

    String::new()
}

/// A command item for the interactive picker
#[derive(Clone)]
struct CommandItem {
    name: String,
    description: String,
    kind: String, // "tool" or "skill"
}

/// Collect all available tools and skills as command items
fn collect_command_items(paths: &Paths) -> Vec<CommandItem> {
    let mut items = Vec::new();

    // Collect built-in tools
    for (_category, tools) in BUILTIN_TOOLS {
        for (name, desc) in *tools {
            items.push(CommandItem {
                name: name.to_string(),
                description: desc.to_string(),
                kind: "tool".to_string(),
            });
        }
    }

    // Collect skills from workspace
    let skills = scan_skill_dirs(&paths.skills_dir());
    for (name, desc) in skills {
        items.push(CommandItem {
            name,
            description: if desc.is_empty() {
                "Skill".to_string()
            } else {
                desc
            },
            kind: "skill".to_string(),
        });
    }

    // Sort by kind (tools first) then by name
    items.sort_by(|a, b| {
        if a.kind != b.kind {
            a.kind.cmp(&b.kind) // tools before skills
        } else {
            a.name.cmp(&b.name)
        }
    });

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockcell_core::config::AgentProfileConfig;
    use std::path::PathBuf;

    #[test]
    fn test_resolve_agent_context_defaults_to_default_agent() {
        let config = Config::default();
        let paths = Paths::with_base(PathBuf::from("/tmp/blockcell"));

        let resolved = resolve_agent_context(&config, &paths, None, None)
            .expect("default agent should resolve");

        assert_eq!(resolved.agent_id, "default");
        assert_eq!(resolved.session, "cli:default");
        assert_eq!(
            resolved.paths.workspace(),
            PathBuf::from("/tmp/blockcell/workspace")
        );
    }

    #[test]
    fn test_resolve_agent_context_uses_named_agent_paths_and_session() {
        let mut config = Config::default();
        config.agents.list.push(AgentProfileConfig {
            id: "ops".to_string(),
            enabled: true,
            model: Some("deepseek-chat".to_string()),
            provider: Some("deepseek".to_string()),
            ..AgentProfileConfig::default()
        });
        let paths = Paths::with_base(PathBuf::from("/tmp/blockcell"));

        let resolved = resolve_agent_context(&config, &paths, Some("ops"), None)
            .expect("named agent should resolve");

        assert_eq!(resolved.agent_id, "ops");
        assert_eq!(resolved.session, "cli:ops");
        assert_eq!(
            resolved.paths.workspace(),
            PathBuf::from("/tmp/blockcell/agents/ops/workspace")
        );
        assert_eq!(
            resolved.config.agents.defaults.provider.as_deref(),
            Some("deepseek")
        );
        assert_eq!(resolved.config.agents.defaults.model, "deepseek-chat");
    }

    #[test]
    fn test_resolve_agent_context_preserves_explicit_session() {
        let mut config = Config::default();
        config.agents.list.push(AgentProfileConfig {
            id: "ops".to_string(),
            enabled: true,
            ..AgentProfileConfig::default()
        });
        let paths = Paths::with_base(PathBuf::from("/tmp/blockcell"));

        let resolved = resolve_agent_context(&config, &paths, Some("ops"), Some("custom:thread"))
            .expect("named agent with explicit session should resolve");

        assert_eq!(resolved.session, "custom:thread");
    }

    #[test]
    fn test_resolve_agent_context_rejects_unknown_agent() {
        let config = Config::default();
        let paths = Paths::with_base(PathBuf::from("/tmp/blockcell"));

        let err = resolve_agent_context(&config, &paths, Some("ops"), None)
            .expect_err("unknown agent should fail");

        assert!(err.to_string().contains("Unknown agent 'ops'"));
    }
}
