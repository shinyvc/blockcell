# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.7] - 2026-06-28

### Added
- Added ModelRouter intelligent routing and connection-phase fallback with `manual`, `cost_optimized`, `quality_first`, and `latency_first` strategies.
- Added the Tool Policy execution policy system with tool-name globs, `|` multi-patterns, channel/path conditions, `allow` / `ask` / `deny` decisions, inherited rule groups, and simulation mode.
- Added global token and cost budget controls with per-session LLM usage tracking to prevent runaway tasks.
- Added the Steering Channel for injecting user messages into an active agent turn while it is still running.
- Added Agent lifecycle hooks for `session_start`, `user_prompt`, `pre_tool_use`, `post_tool_use`, and `agent_stop`, configurable through `~/.blockcell/hooks.yaml`.
- Added SHA-256 hash-chain verification for audit logs, plus SessionStart, SessionEnd, ProviderCall, and BudgetEvent audit events.
- Added on-demand MCP tool discovery: when many MCP tools are allowed, the model sees `mcp_search_tools` while remote tools stay executable but hidden from the default system prompt.
- Added GitHub Actions CI and multi-platform release build workflow.

### Changed
- Updated 2026-06 provider and default model presets across DeepSeek, OpenAI, Anthropic, Gemini, Ollama, GLM, Qwen, and related model families.
- Switched WebUI pages to `React.lazy` + `Suspense` chunks and improved the system-events button layout.
- Reworked Gateway event broadcasting around lightweight `WsEventRouting`, avoiding route-time copies of large content/token payloads.
- Reduced repeated token estimation, history cloning, and per-line allocation in streaming/read paths.
- Made Ghost config hot reload and Cron sync skip full disk reads and parsing when config mtime is unchanged.
- Moved SQLite memory query/write/delete and maintenance operations into `spawn_blocking` to reduce Tokio worker blocking.
- Continued splitting large modules by responsibility across runtime, agent, gateway, config, openai, memory, evolution, and consolidator code.
- Standardized the workspace on Rust 1.85.

### Fixed
- Fixed Gateway API token exposure through URL `?token=` on regular APIs; query tokens are now limited to WebSocket and required file download/serve paths.
- Fixed WebSocket / outbound broadcast session isolation issues and prevented one WS client from approving another session's pending confirmation request.
- Fixed file read/upload size limits, local image serve boundaries, multibyte UTF-8 URL decoding, CLI Unicode cursor editing, and Home/End/Delete key handling.
- Fixed `http_request` SSRF risk, `exec_local` / `exec_skill_script` path escapes, dangerous `rm` command bypasses, timeout orphan processes, and symlink write escapes.
- Fixed Ghost review TOCTOU/path traversal issues, main-flow blocking, throttle leakage, and accidental scanning of `.snapshots` / `.skill_file_store.lockdir`.
- Fixed concurrency, atomicity, transaction commit, recovery, lock-release, budget, and Unicode panic issues across Dream / Session / Auto / Compact memory paths.
- Fixed skill evolution and core evolution state-machine, rollback, staging-path, cross-workflow contamination, infinite retry, Python syntax check, and code-fence injection issues.
- Fixed OpenAI-compatible streaming tool calls, MCP short-name tool loading, hash algorithm use, browser navigation races, CDP listener leaks, and semver prerelease parsing.
- Fixed Windows atomic replacement, path separators, temporary cleanup, cross-process lock deletion, config loss, and schema consistency issues.

### Docs
- Added ModelRouter routing and fallback documentation.
- Added Hook lifecycle event system documentation.
- Updated README / README.en with v0.1.7 security, audit, routing, and hook capabilities.
- Added v0.1.7 release notes.

## [0.1.6] - 2026-05-09

### Added
- Added the Ghost Native learning loop, capturing reusable lessons at turn end, pre-compress, session rotation, session end, delegation end, and evolution success boundaries, then persisting durable knowledge into `USER.md`, `MEMORY.md`, and workspace skills.
- Added a Ghost learning ledger and background review loop for episode audit, review runs, and restricted tool actions without blocking the main assistant response.
- Added Typed Agent and custom Agent loading from user-level and project-level Markdown definitions, including tool scope, model, skill, MCP, one-shot, permission, and background execution settings.
- Added multi-agent task improvements: checkpoints, chained AbortToken cancellation, progress events, task persistence, and improved result injection.
- Added a unified slash-command system for `/help`, `/tasks`, `/skills`, `/tools`, `/learn`, `/clear`, `/compact`, `/session-metrics`, `/log`, and shared Gateway/Channel handling.
- Added 7-layer memory metrics, circuit breakers, and the `/session-metrics` observability entry point.
- Added 30+ configurable memory and compression thresholds in `config.json5`.
- Added OpenClaw skill parsing, gbrain skill compatibility, and the optional RabitQ vector index backend.

### Changed
- Upgraded the default DeepSeek path toward DeepSeek V4 Pro, including 1M context defaults, `reasoningEffort`, and thinking/reasoning request parameters.
- Improved WebUI streaming for thinking/reasoning content, connection state handling, LLM defaults, and agent progress events.
- Unified skill loading, CLI completion, Gateway skills/search, and SkillIndex around recursive skill pack/category scanning and meta-only skill visibility.
- Unified skill description loading as `meta.yaml`/`meta.json` → `SKILL.md` frontmatter → `SKILL.md` body, fixing YAML multiline descriptions and search ranking gaps.
- Migrated core evolution and skill evolution to durable workflows backed by SQLite workflow records, claim/lease, step checkpoints, and background workers.
- Kept SQLite + FTS5 as the canonical memory store while adding optional RabitQ semantic vector recall.
- Hardened the learning write path with unified security scanning, write guards, snapshots, and undo support.

### Fixed
- Fixed DeepSeek DSML tool-call parsing, cross-delta filtering, and missing `reasoning_content` errors.
- Fixed missing thinking/reasoning display, duplicated CLI output, and missing user/LLM log persistence.
- Fixed deep skill-pack category scanning, composite-name truncation, invisible child skills, and accidental empty-skill registration.
- Fixed core evolution ticks blocking the Agent select loop and delaying user input handling.
- Fixed multi-agent task lifecycle, forked subagent cancellation, result injection, task persistence, and Auto Memory trigger logic.
- Fixed Windows config atomic replacement, Python script execution, colon-containing session IDs, and versioning path separators.
- Fixed memory compaction divide-by-zero panics, UTF-8 boundary panics for Chinese text, config propagation gaps, SQL injection risks, and learning write-safety issues.

### Docs
- Added Ghost Native learning design documentation.
- Added design docs for unified slash commands, session metrics, OpenClaw compatibility, configurable memory thresholds, log commands, intent classifier/loadAllTools, and the integrated learning framework.
- Updated Ghost Maintenance docs to clarify its boundary with embedded Ghost learning.

## [0.1.5] - 2026-04-05

### Added
- Unified skill runtime with `rhai` script execution support.
- Skill versioning, auditing, evolution, and service management improvements.
- New Weixin, QQ, and NapCatQQ channel support.
- Memory vector indexing support.
- Expanded WebUI interaction for chat, evolution, and connection states.
- Additional docs for skills, CLI, provider configuration, MCP servers, and path access policy.

### Changed
- Simplified the skill execution model and history handling.
- Improved message/tool call flow and runtime consistency across core modules.
- Enhanced WeCom long-connection support and channel startup/status display.
- Optimized WebUI chat UX, message rendering, and frontend test coverage.
- Updated cron/timezone handling for scheduled tasks.

### Fixed
- Fixed provider compatibility issues and configuration edge cases.
- Fixed gateway/agent/scheduler stability issues and duplicated output problems.
- Fixed path parsing, default model invocation, and media handling bugs.
- Fixed WebUI lag when the gateway disconnects.
- Fixed memory rule inconsistencies across storage, tools, and gateway.

### Docs
- Added and updated a large set of Chinese and English docs for skills, channels, memory, provider configuration, and CLI usage.
- Added workflow/rules documentation for skill development.

## [0.1.4] - 2026-03-25

### Added
- Initial public release for the current tracked line of BlockCell.
- Core agent, provider, storage, scheduler, channels, and skills workspace crates.
- Basic WebUI and gateway integration.
