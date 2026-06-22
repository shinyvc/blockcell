//! 终端交互式输入与命令选择器（command picker）相关的 TUI 逻辑。
//!
//! 本模块从 `commands/agent.rs` 抽离，集中放置原本与 agent 业务逻辑正交的
//! 终端渲染、grapheme 宽度计算、命令/技能补全列表收集等纯 UI 代码。
//! 仅 `read_line_with_command_picker`、`short_task_id`、`clear_prompt_line`、
//! `restore_prompt_line` 对父模块可见，其余均为本模块内部实现。

use blockcell_core::{InboundMessage, Paths};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use tokio::sync::mpsc;
use unicode_segmentation::UnicodeSegmentation;

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
                "Multimodal image understanding (GPT-5.5/Claude/Gemini)",
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
pub(super) fn read_line_with_command_picker(
    paths: &Paths,
    stdout: &mut std::io::Stdout,
    _session: &str,
    _stdin_tx: &mpsc::Sender<InboundMessage>,
    current_input: &std::sync::Mutex<(String, usize)>,
    shutdown_flag: &std::sync::atomic::AtomicBool,
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
                            shutdown_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                            return String::new();
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
                    KeyCode::Tab
                        // Select current item in picker
                        if showing_picker && visible_count > 0 => {
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
                    KeyCode::Up
                        if showing_picker && visible_count > 0 && selected_index > 0 => {
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
                    KeyCode::Down
                        if showing_picker && visible_count > 0 => {
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
                    KeyCode::Backspace
                        if cursor_pos > 0 => {
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
                    KeyCode::Left
                        if cursor_pos > 0 => {
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
                    KeyCode::Home
                        if cursor_pos > 0 => {
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
                    KeyCode::Esc
                        if showing_picker => {
                            clear_suggestions(prev_visible_limit, &input, stdout, cursor_pos);
                            prev_visible_limit = 0;
                            showing_picker = false;
                            visible_count = 0;
                            command_start_pos = None;
                            render_input_line(&input, stdout, cursor_pos);
                            use std::io::Write;
                            let _ = stdout.flush();
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
pub(super) fn short_task_id(task_id: &str, max_chars: usize) -> String {
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
pub(super) fn clear_prompt_line(
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
pub(super) fn restore_prompt_line(
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
#[allow(clippy::too_many_arguments)]
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
