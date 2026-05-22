use std::collections::HashSet;

pub(crate) struct PromptSkillExecutor;

impl PromptSkillExecutor {
    pub(crate) fn resolve_allowed_tool_names(
        skill_tools: &[String],
        available_tool_names: &HashSet<String>,
    ) -> Vec<String> {
        let mut tool_names = skill_tools
            .iter()
            .filter_map(|name| Self::resolve_available_tool_name(name, available_tool_names))
            .filter(|name| Self::is_tool_allowed(name, available_tool_names))
            .collect::<Vec<_>>();
        tool_names.sort();
        tool_names.dedup();
        tool_names
    }

    pub(crate) fn is_tool_allowed(tool_name: &str, available_tool_names: &HashSet<String>) -> bool {
        available_tool_names.contains(tool_name) && !Self::is_blocked_tool(tool_name)
    }

    fn is_blocked_tool(tool_name: &str) -> bool {
        matches!(tool_name, "spawn")
    }

    fn resolve_available_tool_name(
        requested_name: &str,
        available_tool_names: &HashSet<String>,
    ) -> Option<String> {
        if available_tool_names.contains(requested_name) {
            return Some(requested_name.to_string());
        }

        // 兼容从 MCP 原生生态导入的技能
        // 而 BlockCell 暴露 MCP 工具时会使用 `<server>__<tool>` 格式，
        if requested_name.contains("__") {
            return None;
        }

        let mut candidates = available_tool_names.iter().filter(|available_name| {
            available_name
                .split_once("__")
                .is_some_and(|(_, tool_name)| tool_name == requested_name)
        });
        let candidate = candidates.next()?.clone();
        if candidates.next().is_some() {
            return None;
        }
        Some(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::PromptSkillExecutor;
    use std::collections::HashSet;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn resolves_exact_tool_names() {
        let available = set(&["read_file", "gbrain__artifact_upload"]);
        let tools = PromptSkillExecutor::resolve_allowed_tool_names(
            &["read_file".to_string(), "missing_tool".to_string()],
            &available,
        );

        assert_eq!(tools, vec!["read_file".to_string()]);
    }

    #[test]
    fn resolves_unique_unqualified_mcp_tool_name() {
        let available = set(&[
            "read_file",
            "gbrain__artifact_upload",
            "gbrain__artifact_query",
        ]);
        let tools = PromptSkillExecutor::resolve_allowed_tool_names(
            &["artifact_upload".to_string(), "artifact_query".to_string()],
            &available,
        );

        assert_eq!(
            tools,
            vec![
                "gbrain__artifact_query".to_string(),
                "gbrain__artifact_upload".to_string()
            ]
        );
    }

    #[test]
    fn does_not_resolve_ambiguous_unqualified_mcp_tool_name() {
        let available = set(&["gbrain__query", "github__query"]);
        let tools =
            PromptSkillExecutor::resolve_allowed_tool_names(&["query".to_string()], &available);

        assert!(tools.is_empty());
    }

    #[test]
    fn keeps_blocked_tools_out_after_alias_resolution() {
        let available = set(&["spawn", "gbrain__artifact_upload"]);
        let tools = PromptSkillExecutor::resolve_allowed_tool_names(
            &["spawn".to_string(), "artifact_upload".to_string()],
            &available,
        );

        assert_eq!(tools, vec!["gbrain__artifact_upload".to_string()]);
    }
}
