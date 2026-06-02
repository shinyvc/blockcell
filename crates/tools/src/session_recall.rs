use crate::{Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use blockcell_core::Result;
use serde_json::{json, Value};

/// Retrieves a previously cached large response by its cache ID.
///
/// When the LLM returns a long numbered list or table, the runtime caches the full
/// content and replaces the history entry with a compact stub containing a ref_id.
/// Call this tool to get the full content back when the user references a specific item.
pub struct SessionRecallTool;

#[async_trait]
impl Tool for SessionRecallTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "session_recall".to_string(),
            description: "从当前会话缓存中取回之前返回的完整列表/表格内容。\
                当历史消息中出现 [已缓存N条结果，ID: ref:XXXXXX] 时，使用此工具获取完整内容。\
                场景：用户询问某个列表的第N条、要求展示完整结果、引用之前搜索/查询的数据。"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "缓存内容的ID，格式为 ref:XXXXXX 或直接输入 XXXXXX（8位十六进制）"
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
            调用此工具传入对应ID即可取回完整列表内容。用户说「第X条是什么」「完整列表」「显示全部」时优先调用此工具。"
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
                "hint": "请提供缓存ID，例如: ref:a3f8c21e"
            }));
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
