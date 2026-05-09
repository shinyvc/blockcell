# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
