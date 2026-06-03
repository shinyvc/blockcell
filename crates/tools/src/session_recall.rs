use crate::{Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use blockcell_core::Result;
use serde_json::{json, Value};

/// 通过缓存 ID 检索之前缓存的助手响应（列表/表格）。
///
/// 当 LLM 返回长编号列表或表格时，运行时会缓存完整内容
/// 并用包含 ref_id 的紧凑存根替换历史条目。
/// 当用户引用特定条目时，调用此工具获取完整内容。
///
/// ## 当前范围
/// 此工具恢复缓存的助手响应（assistant response cache），
/// 也支持通过 `tool:{id}` 格式的 ID 恢复磁盘持久化的大工具输出（`<persisted-output>` 存根）。
pub struct SessionRecallTool;

#[async_trait]
impl Tool for SessionRecallTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "session_recall".to_string(),
            description: "从当前会话缓存中取回之前返回的完整列表/表格内容。\
                当历史消息中出现 [已缓存N条结果，ID: ref:XXXXXX] 时，使用此工具获取完整内容。\
                也支持通过 `tool:{id}` 格式的 ID 恢复磁盘持久化的大工具输出（`<persisted-output>` 存根）。\
                场景：用户询问某个列表的第N条、要求展示完整结果、引用之前搜索/查询的数据。"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "缓存内容的ID，格式为 ref:XXXXXX 或直接输入 XXXXXX（8位十六进制），\
                            也支持 tool:{id} 格式恢复持久化工具输出"
                    }
                },
                "required": ["id"]
            }),
        }
    }

    fn validate(&self, params: &Value) -> Result<()> {
        if params.get("id").and_then(|v| v.as_str()).is_none() {
            return Err(blockcell_core::Error::Tool(
                "session_recall: 缺少必填参数 'id'".to_string(),
            ));
        }
        Ok(())
    }

    fn prompt_rule(&self, _ctx: &crate::PromptContext) -> Option<String> {
        Some(
            "- **session_recall**: 当历史消息中出现 `[已缓存N条结果，ID: ref:XXXXXX]` 时，\
            调用此工具传入对应ID即可取回完整列表内容。\
            也支持 `tool:{id}` 格式恢复已持久化的工具输出（`<persisted-output>` 存根）。\
            用户说「第X条是什么」「完整列表」「显示全部」时优先调用此工具。"
                .to_string(),
        )
    }

    async fn execute(&self, ctx: ToolContext, params: Value) -> Result<Value> {
        let id = params
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if id.is_empty() {
            return Ok(json!({
                "error": "缺少参数 id",
                "hint": "请提供缓存ID，例如: ref:a3f8c21e 或 tool:tool-xxx"
            }));
        }

        // 检查是否是持久化工具结果 ID
        if id.starts_with("tool:") {
            let tool_id = &id[5..]; // 去掉 "tool:" 前缀
                                    // 路径安全验证：拒绝包含路径遍历字符的 tool_id
            if tool_id.is_empty()
                || tool_id.contains("..")
                || tool_id.contains('/')
                || tool_id.contains('\\')
                || tool_id.contains('\0')
            {
                return Ok(json!({
                    "tool_id": tool_id,
                    "error": "无效的 tool_id，包含不安全字符",
                    "status": "invalid"
                }));
            }
            let workspace_dir = &ctx.workspace;
            let output_path = workspace_dir
                .join(".tool_results")
                .join(tool_id)
                .join("output.txt");
            match tokio::fs::read_to_string(&output_path).await {
                Ok(content) => {
                    return Ok(json!({
                        "tool_id": tool_id,
                        "content": content,
                        "status": "found"
                    }));
                }
                Err(_) => {
                    return Ok(json!({
                        "tool_id": tool_id,
                        "error": "未找到持久化工具输出，可能已被清理",
                        "status": "not_found"
                    }));
                }
            }
        }

        let cache = match &ctx.response_cache {
            Some(c) => c,
            None => {
                return Ok(json!({
                    "error": "响应缓存不可用",
                    "hint": "当前会话未启用响应缓存"
                }));
            }
        };

        let result_json = cache.recall_json(&ctx.session_key, &id);
        // Parse and return as Value so it doesn't get double-encoded
        Ok(serde_json::from_str(&result_json).unwrap_or_else(|_| json!({"raw": result_json})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema() {
        let tool = SessionRecallTool;
        let schema = tool.schema();
        assert_eq!(schema.name, "session_recall");
        assert!(schema.parameters.get("properties").is_some());
    }

    #[test]
    fn test_validate_ok() {
        let tool = SessionRecallTool;
        assert!(tool.validate(&json!({"id": "ref:a1b2c3d4"})).is_ok());
    }

    #[test]
    fn test_validate_missing_id() {
        let tool = SessionRecallTool;
        assert!(tool.validate(&json!({})).is_err());
    }
}
