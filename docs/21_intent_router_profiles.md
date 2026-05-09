# intentRouter 多智能体配置指南

`intentRouter` 现在是 blockcell 中 **意图到工具映射的唯一配置入口**。如果你想让不同的 agent 拥有不同的能力范围，推荐把“agent 归属”和“工具编排”拆开配置：

- 在 `agents.list[].intentProfile` 上绑定每个 agent 使用的 profile
- 在 `intentRouter.profiles` 中定义可复用工具集
- 用 `coreTools` 放共享基础工具
- 用 `intentTools` 按意图追加工具
- 用 `denyTools` 从最终结果里移除不允许暴露的工具
- 为 `Chat` 显式写 `{ "inheritBase": false, "tools": [] }`
- 始终配置 `Unknown`
- 如果只是想临时关闭意图分类但保留全量工具，可用 `enabled: false` + `loadAllTools: true`
- 如果内置意图规则不能覆盖你的业务词汇，可用 `intentRules` 追加关键词或正则

> `allowedMcpServers` / `allowedMcpTools` 是 agent 级别的 MCP 可见性白名单，字段名是 camelCase。

## 先理解这几个规则

1. `agents.list[].intentProfile` 的优先级最高。
2. 如果 agent 没写 `intentProfile`，则回退到 `intentRouter.agentProfiles`。
3. 如果还是找不到，就回退到 `intentRouter.defaultProfile`。
4. `agents.list` 为空时，runtime 会回退到一个隐式的 `default` agent。
5. `intentRouter` 缺失时，blockcell 会自动注入内置默认 router。
6. 如果 `intentRouter.enabled = false` 且 `loadAllTools = false`，runtime 仍然会解析 profile，但最终只使用该 profile 的 `Unknown` 工具集。
7. 如果 `intentRouter.enabled = false` 且 `loadAllTools = true`，runtime 会暴露当前可用的全部工具，再应用当前 profile 的 `denyTools`。
8. 如果 `intentRouter.enabled = true`，`loadAllTools` 会被忽略，仍按意图分类解析工具。
9. 每个 profile 都必须提供 `Unknown`，否则配置校验会失败。

## 关闭分类但保留全量工具

有些场景下你可能不想让意图分类器筛工具，而是希望 LLM 看到当前可用的完整工具集。可以这样配置：

```json
{
  "intentRouter": {
    "enabled": false,
    "loadAllTools": true,
    "defaultProfile": "default",
    "profiles": {
      "default": {
        "coreTools": [],
        "intentTools": {
          "Unknown": []
        },
        "denyTools": ["email", "exec"]
      }
    }
  }
}
```

这种模式下：

- `enabled: false` 表示不走意图分类
- `loadAllTools: true` 表示返回所有当前注册且可用的工具
- `denyTools` 仍然会生效，适合保留全量能力但排除高风险工具
- 如果不设置 `loadAllTools`，默认是 `false`，会回到只暴露 `Unknown` 工具集的保守行为

## 追加自定义意图规则

`intentRules` 用来补充内置分类规则，适合把业务词汇映射到已有意图类别。它不会覆盖内置规则，只会追加命中条件。

```json
{
  "intentRouter": {
    "enabled": true,
    "intentRules": [
      {
        "category": "Finance",
        "keywords": ["资金费率", "合约持仓", "链上净流入"],
        "patterns": ["(?i)funding\\s+rate", "(?i)open\\s+interest"],
        "negative": ["不是行情"],
        "priority": 80
      }
    ]
  }
}
```

字段说明：

| 字段 | 说明 |
|------|------|
| `category` | 必填，必须是已有意图类别，例如 `Finance`、`FileOps`、`WebSearch` |
| `keywords` | 大小写不敏感的关键词，出现即命中 |
| `patterns` | 正则表达式，任意一条匹配即命中；无效正则会被跳过并记录 warning |
| `negative` | 否定关键词，命中后跳过该规则 |
| `priority` | 规则优先级，默认 `60`；当前主要用于规则排序 |

## 示例一：默认助手 + 运维助手

这个例子适合最常见的两人/两角色拆分：

- `default` 负责日常对话、文件操作、网页搜索
- `ops` 负责运维、排障、内部管理类任务

```json
{
  "agents": {
    "list": [
      {
        "id": "default",
        "enabled": true,
        "name": "日常助手",
        "intentProfile": "default",
        "allowedMcpServers": ["github", "sqlite"],
        "allowedMcpTools": ["github__list_issues", "sqlite__query"]
      },
      {
        "id": "ops",
        "enabled": true,
        "name": "运维助手",
        "intentProfile": "ops",
        "allowedMcpServers": ["github"],
        "allowedMcpTools": ["github__list_issues", "github__create_issue"]
      }
    ]
  },
  "intentRouter": {
    "enabled": true,
    "defaultProfile": "default",
    "profiles": {
      "default": {
        "coreTools": [
          "read_file",
          "write_file",
          "list_dir",
          "exec",
          "web_search",
          "web_fetch",
          "memory_query",
          "memory_upsert",
          "toggle_manage",
          "message",
          "agent_status"
        ],
        "intentTools": {
          "Chat": { "inheritBase": false, "tools": [] },
          "FileOps": ["edit_file", "file_ops", "data_process", "office_write"],
          "WebSearch": ["browse", "http_request"],
          "Unknown": ["edit_file", "file_ops", "office_write", "http_request", "browse"]
        }
      },
      "ops": {
        "coreTools": [
          "read_file",
          "list_dir",
          "exec",
          "web_search",
          "web_fetch",
          "message",
          "agent_status"
        ],
        "intentTools": {
          "DevOps": ["network_monitor", "encrypt", "http_request", "edit_file", "file_ops"],
          "Organization": ["cron", "memory_maintenance", "list_skills"],
          "Unknown": ["http_request", "browse"]
        },
        "denyTools": ["email"]
      }
    }
  }
}
```

### 这个例子怎么理解

- `default` 的 `Chat` 不继承基础工具，所以闲聊不会意外带出工具能力。
- `default` 的 `FileOps` 和 `WebSearch` 分别对应文件操作和网页搜索场景。
- `ops` 的 `DevOps` 允许排障、加密、文件编辑和 HTTP 调试工具。
- `ops` 额外配置了 `denyTools: ["email"]`，即使别处把 `email` 放进来了，这个 profile 也会把它移除。
- `allowedMcpServers` / `allowedMcpTools` 只影响当前 agent 能看到哪些 MCP 资源，不影响 `intentRouter.profiles` 里的工具编排。

## 示例二：客服 + 金融分析 + 平台管理员

这个例子更适合“前台接待 / 财务分析 / 平台运维”拆分：

- `support` 负责日常客服和消息回复
- `finance` 负责行情、图表、告警、日报
- `admin` 负责系统控制和平台维护

```json
{
  "agents": {
    "list": [
      {
        "id": "support",
        "enabled": true,
        "name": "客服",
        "intentProfile": "support",
        "allowedMcpServers": ["sqlite"],
        "allowedMcpTools": ["sqlite__query"]
      },
      {
        "id": "finance",
        "enabled": true,
        "name": "金融分析",
        "intentProfile": "finance",
        "allowedMcpServers": ["market-data", "news"],
        "allowedMcpTools": ["market-data__quote", "news__search"]
      },
      {
        "id": "admin",
        "enabled": true,
        "name": "平台管理员",
        "intentProfile": "admin",
        "allowedMcpServers": ["github"],
        "allowedMcpTools": ["github__list_issues", "github__create_issue"]
      }
    ]
  },
  "intentRouter": {
    "enabled": true,
    "defaultProfile": "support",
    "profiles": {
      "support": {
        "coreTools": [
          "read_file",
          "list_dir",
          "message",
          "web_search",
          "web_fetch",
          "memory_query",
          "memory_upsert"
        ],
        "intentTools": {
          "Chat": { "inheritBase": false, "tools": [] },
          "Communication": ["email", "message", "http_request"],
          "Organization": ["list_tasks", "cron", "community_hub", "memory_maintenance"],
          "Unknown": ["message", "http_request"]
        }
      },
      "finance": {
        "coreTools": ["read_file", "web_search", "web_fetch", "message", "agent_status"],
        "intentTools": {
          "Finance": [
            "http_request",
            "data_process",
            "chart_generate",
            "alert_rule",
            "stream_subscribe",
            "knowledge_graph",
            "cron",
            "office_write",
            "browse"
          ],
          "WebSearch": ["browse", "http_request"],
          "Unknown": ["http_request", "browse"]
        },
        "denyTools": ["email"]
      },
      "admin": {
        "coreTools": ["read_file", "write_file", "list_dir", "exec", "toggle_manage", "message", "agent_status"],
        "intentTools": {
          "SystemControl": ["system_info", "app_control", "camera_capture", "browse", "image_understand", "termux_api"],
          "DevOps": ["network_monitor", "encrypt", "http_request", "edit_file", "file_ops"],
          "Unknown": ["edit_file", "file_ops", "http_request"]
        }
      }
    }
  }
}
```

### 这个例子怎么理解

- `support` 代理的是高频聊天和工单回复，所以 `Chat` 仍然是零工具。
- `finance` 专门把行情、告警、图表和日报工具聚在一起，适合做“金融分析 Agent”。
- `admin` 允许系统控制和运维类工具，但没有金融类能力。
- 如果你把 `finance` 设成某个渠道的 owner，那么这类消息就会优先落到金融 profile 上。
- 如果 `intentRouter.enabled = false`，这三个 profile 仍会存在，但每个 agent 最终只会拿到自己 profile 的 `Unknown` 工具集。

> 以上 MCP server / tool 名称只是示例，请替换成你自己 `mcp.json` + `mcp.d` 里真实存在的名称。

## 解析顺序

最终工具集合按下面顺序得到：

1. 选择 agent 对应的 profile：先看 `agents.list[].intentProfile`，再看 `intentRouter.agentProfiles`，最后回退 `defaultProfile`
2. 根据 intent 合并 `coreTools` 与 `intentTools`
3. 叠加运行时强制工具（如 ghost 所需工具）
4. 应用 `denyTools`
5. 应用运行时其它工具开关或可见性过滤

## 兼容行为

- 如果 `intentRouter` 整段缺失，blockcell 会自动注入内置默认 router
- `agents.list[].intentProfile` 的优先级高于 `intentRouter.agentProfiles`；后者主要用于兼容旧配置
- 如果 `agents.list` 为空，runtime 会回退到一个隐式的 `default` agent
- `allowedMcpServers` / `allowedMcpTools` 是 agent 级别的 MCP 可见性白名单，字段名是 camelCase

## 排查建议

可以先运行：

```bash
blockcell status
blockcell doctor
```

重点看：

- 当前 agent 绑定到哪个 profile
- `intentRouter` 是否通过校验
- 是否引用了未注册的工具名
- `Unknown` 是否有配置
- 如果 profile 里引用了 MCP 工具，MCP server / tool 名称是否与当前 `mcp.json` + `mcp.d` 合并结果一致
