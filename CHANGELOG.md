# 更新日志

所有值得注意的变更都会记录在此文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)，
并遵循 [语义化版本](https://semver.org/spec/v2.0.0.html)。

## [0.1.6] - 2026-05-09

### 新增
- 新增 Ghost Native 学习闭环：在 turn end、pre-compress、session rotate、session end、delegation end 等边界捕获可复用经验，并沉淀到 `USER.md`、`MEMORY.md` 与 workspace skills。
- 新增 Ghost 学习审计账本与后台 review，记录 episode、review run 和受限工具动作，后台学习失败不会阻塞主对话。
- 新增 Typed Agent 与自定义 Agent 加载能力，支持用户级和项目级 Markdown agent 定义、工具范围、模型、技能、MCP、one-shot、权限模式和后台执行配置。
- 新增多智能体任务增强：checkpoint、AbortToken 链式取消、任务进度事件、任务持久化和结果注入改进。
- 新增统一斜杠命令体系，覆盖 `/help`、`/tasks`、`/skills`、`/tools`、`/learn`、`/clear`、`/compact`、`/session-metrics`、`/log` 等命令，并支持 Gateway/Channel 统一处理。
- 新增 7 层记忆系统指标、熔断器和 `/session-metrics` 观察入口。
- 新增 30+ 个记忆与压缩阈值配置项，可通过 `config.json5` 调整。
- 新增 OpenClaw 技能格式解析、gbrain skills 兼容加载，以及 RabitQ 可选向量索引后端。

### 调整
- DeepSeek 默认体验升级到 DeepSeek V4 Pro 方向，支持 1M 上下文、`reasoningEffort` 配置和 thinking/reasoning 参数注入。
- WebUI 支持 thinking/reasoning 内容流式显示，并改进连接状态、LLM 页面默认值和 agent progress 事件处理。
- skill 加载、CLI 自动补全、Gateway skills/search 与 SkillIndex 统一支持技能包/category 递归扫描和 meta-only skill 可见。
- 技能描述读取统一为 `meta.yaml`/`meta.json` → `SKILL.md` frontmatter → `SKILL.md` 正文，修复多行 YAML description 与搜索权重问题。
- 核心进化与 skill evolution 迁移到 durable workflow，使用 SQLite workflow store、claim/lease、step checkpoint 和后台 worker 推进长流程，避免阻塞 Agent 主循环。
- 记忆系统继续以 SQLite + FTS5 为主存储，并可选接入 RabitQ 做语义向量召回。
- 自学习写入链路增加统一安全扫描、写入守护、快照和撤销能力。

### 修复
- 修复 DeepSeek DSML 工具调用标签解析、跨 delta 分片过滤和 `reasoning_content` 丢失导致的请求错误。
- 修复 WebUI 与 CLI 中 thinking/reasoning 内容不显示、重复输出或日志缺失的问题。
- 修复 skill pack 深层 category、composite name 截断、子 skill 不可见、空目录误注册为 skill 等问题。
- 修复核心进化 tick 阻塞 select 循环导致 Agent 无法响应用户输入的问题。
- 修复多智能体任务生命周期、forked 子代理取消链路、结果注入、任务持久化和 Auto Memory 触发判断问题。
- 修复 Windows 配置原子写入、Python 脚本执行、session_id 冒号路径、versioning 路径分隔符等兼容性问题。
- 修复记忆压缩比例除零 panic、中文文本 UTF-8 边界 panic、配置传播缺失、SQL 注入/安全扫描/写入守护相关问题。

### 文档
- 新增 Ghost Native 学习设计文档。
- 新增统一斜杠命令、session metrics、OpenClaw 兼容、记忆阈值配置、日志命令、intent classifier/loadAllTools、自学习框架等设计文档。
- 更新 Ghost Maintenance 文档，明确它与嵌入式 Ghost 学习的边界。

## [0.1.5] - 2026-04-05

### 新增
- 统一技能运行时，支持 `rhai` 脚本执行链路。
- 增强技能版本管理、审计、演化与服务管理能力。
- 新增 Weixin、QQ 和 NapCatQQ 频道支持。
- 增加 memory 向量索引支持。
- 扩展 WebUI 在聊天、演化与连接状态方面的交互能力。
- 补充技能、CLI、provider 配置、MCP server 和路径访问策略等文档。

### 调整
- 简化技能执行模型与历史处理逻辑。
- 优化消息与工具调用流程，提升核心模块运行一致性。
- 增强 WeCom 长连接支持，并优化频道启动与状态展示。
- 优化 WebUI 聊天体验、消息渲染和前端测试覆盖。
- 更新定时任务的 cron / 时区处理逻辑。

### 修复
- 修复 provider 兼容性问题和部分配置边界情况。
- 修复 gateway / agent / scheduler 的稳定性问题以及重复输出问题。
- 修复路径解析、默认模型调用和媒体处理相关 bug。
- 修复 gateway 断开时 WebUI 卡顿问题。
- 修复 storage / tools / gateway 之间的 memory 规则不一致问题。

### 文档
- 补充并更新了大量中文与英文文档，覆盖技能、渠道、memory、provider 配置和 CLI 使用。
- 新增技能开发相关的 workflow / rules 文档。

## [0.1.4] - 2026-03-25

### 新增
- 当前追踪分支的首次公开版本。
- 核心 agent、provider、storage、scheduler、channels 和 skills 工作区 crate。
- 基础 WebUI 与 gateway 集成。
