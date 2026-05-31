use serde_json::Value;

/// 构建工具描述文本，注入到 system prompt 中。
/// 用于不支持原生 tool calling 的模型（回退到文本格式）。
pub fn build_tools_prompt(tools: &[Value]) -> String {
    let mut s = String::new();
    s.push_str("\n\n## Available Tools\n");
    s.push_str("You MUST use tools to accomplish tasks. To call a tool, output a `<tool_call>` block with JSON inside.\n");
    s.push_str("You may call multiple tools in one response. Each call must be a separate `<tool_call>` block.\n\n");
    s.push_str("Format (you MUST follow this exact format):\n```\n<tool_call>\n{\"name\": \"tool_name\", \"arguments\": {\"param1\": \"value1\"}}\n</tool_call>\n```\n\n");
    s.push_str("IMPORTANT RULES:\n");
    s.push_str("- When the user asks you to do something that requires a tool, you MUST output <tool_call> blocks. Do NOT just describe what you would do.\n");
    s.push_str("- After outputting tool calls, STOP and wait for the results. Do NOT guess or fabricate results.\n");
    s.push_str("- If you don't need any tool, just respond normally with text.\n");
    s.push_str("- For web content, use web_fetch. For search, use web_search.\n\n");
    s.push_str("Tools:\n");

    for tool in tools {
        if let Some(func) = tool.get("function") {
            let name = func
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let desc = func
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let params = func.get("parameters").cloned().unwrap_or(Value::Null);
            s.push_str(&format!("### {}\n", name));
            s.push_str(&format!("{}\n", desc));
            if !params.is_null() {
                if let Ok(params_str) = serde_json::to_string_pretty(&params) {
                    s.push_str(&format!("Parameters: {}\n", params_str));
                }
            }
            s.push('\n');
        }
    }
    s
}
