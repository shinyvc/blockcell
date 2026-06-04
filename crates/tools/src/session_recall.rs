use crate::{Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use blockcell_core::Result;
use serde_json::{json, Value};
use std::path::PathBuf;

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
        if let Some(tool_id) = id.strip_prefix("tool:") {
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

            // 对 session_key 做与写入侧 (response_cache::sanitize_session_key) 相同的清洗：
            // 前缀保留 48 字符 ascii 字母数字 + -_，附加 SHA-256 64 位哈希后缀保证唯一性
            // 防止不同会话（如 "a.b" 和 "a-b"）映射到同一目录
            // 直接复用 blockcell_core::stable_hash_session_key，保证与写入侧算法一致
            let session_id: String = {
                let clean: String = ctx
                    .session_key
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .take(48)
                    .collect();
                let hash_suffix = blockcell_core::stable_hash_session_key(&ctx.session_key);
                if clean.is_empty() {
                    format!("session_{hash_suffix}")
                } else {
                    format!("{clean}_{hash_suffix}")
                }
            };

            let session_dir = workspace_dir
                .join(".tool_results")
                .join(&session_id);

            let mut found_content: Option<String> = None;

            // ── 路径 1（新）：精确目录匹配 ──
            // 格式 tool:{tool_id}:{call_uuid}
            // 写入侧 (persist_tool_result / try_persist_large_tool_result) 使用
            // {tool_id}_{call_uuid} 作为目录名，UUID 保证唯一性，无需前缀扫描
            // is_new_format 标记：当 tool_id 包含 ':' 时为新格式（tool:{base}:{uuid}），
            // 精确匹配失败后不应回退到前缀扫描，否则可能静默返回另一轮调用的内容
            let is_new_format = tool_id.contains(':');
            if is_new_format {
                if let Some((base_id, call_uuid)) = tool_id.rsplit_once(':') {
                    if !call_uuid.is_empty() && !base_id.is_empty() {
                        let exact_dir_name = format!("{base_id}_{call_uuid}");
                        let exact_path = session_dir
                            .join(&exact_dir_name)
                            .join("output.txt");
                        if tokio::fs::metadata(&exact_path).await.is_ok() {
                            found_content = tokio::fs::read_to_string(&exact_path).await.ok();
                        }
                    }
                }
            }

            // ── 路径 2（回退）：前缀扫描 + 最新修改时间 ──
            // 仅旧格式 tool:{tool_id}（无 UUID）走此路径。
            // 新格式 tool:{base}:{uuid} 精确匹配失败后直接返回 not_found，
            // 避免文件被清理、ID 拼错或 stub 过期时静默返回另一轮 text_call_0 的内容。
            if !is_new_format && found_content.is_none() {
                let base_tool_id = tool_id;

                if let Ok(mut entries) = tokio::fs::read_dir(&session_dir).await {
                    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
                    let prefix = format!("{base_tool_id}_");

                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        // 匹配 {base_tool_id}_* 前缀
                        if !name_str.starts_with(&prefix) {
                            continue;
                        }
                        let output_file = entry.path().join("output.txt");
                        if tokio::fs::metadata(&output_file).await.is_ok() {
                            if let Ok(meta) = entry.metadata().await {
                                if let Ok(modified) = meta.modified() {
                                    candidates.push((modified, output_file));
                                }
                            }
                        }
                    }

                    if !candidates.is_empty() {
                        // 同一 tool_id 可能被多次调用（跨轮次），按修改时间降序取最新的
                        candidates.sort_by(|a, b| b.0.cmp(&a.0));
                        if let Some((_, path)) = candidates.into_iter().next() {
                            found_content = tokio::fs::read_to_string(&path).await.ok();
                        }
                    }
                }
            }

            // 回退：旧路径格式 .tool_results/{tool_id}/output.txt（向后兼容）
            // 仅旧格式走此路径；新格式精确匹配失败后不执行回退
            if !is_new_format && found_content.is_none() {
                let old_path = workspace_dir
                    .join(".tool_results")
                    .join(tool_id)
                    .join("output.txt");
                found_content = tokio::fs::read_to_string(&old_path).await.ok();
            }

            return match found_content {
                Some(content) => Ok(json!({
                    "tool_id": tool_id,
                    "content": content,
                    "status": "found"
                })),
                None => Ok(json!({
                    "tool_id": tool_id,
                    "error": "未找到持久化工具输出，可能已被清理",
                    "status": "not_found"
                })),
            };
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
