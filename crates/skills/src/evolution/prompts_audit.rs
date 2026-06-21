use super::*;

impl SkillEvolution {
    pub(crate) fn extract_yaml_from_response(&self, response: &str) -> Option<String> {
        fn extract_with_marker(response: &str, marker: &str) -> Option<String> {
            let start = response.find(marker)?;
            let mut i = start + marker.len();

            if i < response.len() {
                let rest = &response[i..];
                let line_end = rest.find('\n').unwrap_or(rest.len());
                if line_end > 0 {
                    i += line_end;
                }
            }

            while i < response.len()
                && (response.as_bytes()[i] == b'\n' || response.as_bytes()[i] == b'\r')
            {
                i += 1;
            }

            let end_rel = response[i..].find("```")?;
            let yaml = &response[i..i + end_rel];
            let trimmed = yaml.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }

        extract_with_marker(response, "```yaml").or_else(|| extract_with_marker(response, "```yml"))
    }

    /// Sanitize content for embedding inside a markdown code fence.
    /// Replaces triple-backtick sequences to prevent code fence escape
    /// (prompt injection via generated script content).
    pub(crate) fn sanitize_for_code_fence(content: &str) -> String {
        content.replace("```", "\u{200B}``\u{200B}`")
    }

    pub(crate) fn build_audit_prompt(
        &self,
        context: &EvolutionContext,
        script_content: &str,
    ) -> Result<String> {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "You are a security auditor for Rhai scripts in the blockcell agent framework.\n\
            Review the following complete script for skill '{}'.\n\n",
            context.skill_name
        ));

        prompt.push_str(&format!(
            "Code:\n```rhai\n{}\n```\n\n",
            Self::sanitize_for_code_fence(script_content)
        ));

        prompt.push_str("\
Check for the following Rhai-specific issues:\n\
1. **Syntax errors**: Is this valid Rhai syntax? (No JS/Python/TS syntax like `class`, `import`, `require`, `const`, `=>`, `async`)\n\
2. **Language correctness**: Uses Rhai idioms (object maps `#{}`, `fn` for functions, `let` for variables)\n\
3. **Infinite loops**: Unbounded `loop {}` or `while true {}` without break conditions\n\
4. **Resource abuse**: Operations that could consume excessive memory or CPU\n\
5. **Data leakage**: Logging sensitive information via `print()`\n\n\
Respond with ONLY a JSON object (no markdown code blocks, no extra text):\n\
{\"passed\": true, \"issues\": []}\n\
or\n\
{\"passed\": false, \"issues\": [{\"severity\": \"error\", \"category\": \"syntax\", \"message\": \"description\"}]}\n");

        Ok(prompt)
    }

    pub(crate) fn build_prompt_only_audit_prompt(
        &self,
        context: &EvolutionContext,
        md_content: &str,
    ) -> Result<String> {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "You are a quality reviewer for SKILL.md documents in the blockcell agent framework.\n\
            Review the following SKILL.md content for skill '{}'.\n\n",
            context.skill_name
        ));

        prompt.push_str(&format!(
            "Content:\n```markdown\n{}\n```\n\n",
            Self::sanitize_for_code_fence(md_content)
        ));

        prompt.push_str("\
Check for the following issues:\n\
1. **Completeness**: Does it describe what the skill does and how to use it?\n\
2. **Clarity**: Are the instructions clear and actionable for an AI agent?\n\
3. **Length**: Is the content at least 100 characters and substantive?\n\
4. **Structure**: Does it have clear sections/headings?\n\n\
Respond with ONLY a JSON object (no markdown code blocks, no extra text):\n\
{\"passed\": true, \"issues\": []}\n\
or\n\
{\"passed\": false, \"issues\": [{\"severity\": \"error\", \"category\": \"completeness\", \"message\": \"description\"}]}\n");

        Ok(prompt)
    }

    pub(crate) fn build_python_generation_prompt(
        &self,
        context: &EvolutionContext,
    ) -> Result<String> {
        let is_manual = matches!(context.trigger, TriggerReason::ManualRequest { .. });
        let mut prompt = String::new();

        prompt.push_str("You are a Python skill developer for the blockcell agent framework.\n");
        prompt.push_str("Your task is to write or improve a SKILL.py file — a Python script\n");
        prompt.push_str("that implements the skill's logic. The script will be executed by the agent via `python3 SKILL.py`.\n\n");

        prompt.push_str("## Requirements\n");
        prompt.push_str("- Use Python 3.8+ compatible syntax\n");
        prompt.push_str("- Read input from stdin (JSON) or command-line arguments\n");
        prompt.push_str("- Output results to stdout (preferably JSON)\n");
        prompt.push_str("- Handle errors gracefully with try/except\n");
        prompt.push_str("- Only use standard library modules or widely available packages\n");
        prompt.push_str("- Include a `if __name__ == '__main__':` block\n\n");

        // Enriched context: project rules, evolution history, adjacent skills
        let enriched = self.gather_evolution_context(context);
        prompt.push_str(&self.format_enriched_context(&enriched));

        if is_manual {
            if let TriggerReason::ManualRequest { ref description } = context.trigger {
                if Self::is_openclaw_import_description(description) {
                    prompt.push_str(&format!("## Task\n{}\n\n", description));
                } else {
                    prompt.push_str(&format!(
                        "## Task\nCreate or improve a Blockcell SKILL.py for: {}\n\n",
                        description
                    ));
                }
            }
        } else {
            prompt.push_str(&format!(
                "## Task\nFix the existing Blockcell SKILL.py for skill '{}' to address the following issue. Preserve the skill's purpose and change only what is needed to fix it.\n\n",
                context.skill_name
            ));
            if let Some(error) = &context.error_stack {
                prompt.push_str(&format!(
                    "## Issue\n```\n{}\n```\n\n",
                    Self::sanitize_for_code_fence(error)
                ));
            }
        }

        if let Some(snippet) = &context.source_snippet {
            prompt.push_str(&format!(
                "## Current SKILL.py Content\n```python\n{}\n```\n\n",
                Self::sanitize_for_code_fence(snippet)
            ));
        }

        prompt.push_str("## Output Format\n");
        prompt.push_str("Generate the COMPLETE SKILL.py content.\n");
        prompt.push_str("If the skill can directly produce final user-facing text, return `display_text`. Otherwise return `summary_data` as a lightweight structured summary for runtime/LLM polishing. Do NOT place complete webpages, long markdown, or large raw logs into `summary_data`.\n");
        prompt.push_str("If this skill also needs a post-execution result polishing step, you may optionally add `## Summary {#summary}` to the paired SKILL.md. Treat that section as a soft hint for runtime summary handling, not a hard requirement.\n");
        prompt.push_str("Output the Python code in a ```python code block.\n");
        prompt.push_str(
            "If you output `meta.yaml`, it must follow the minimal meta rules below.\n\n",
        );
        prompt.push_str(Self::trigger_rules_prompt());
        prompt.push_str("The script must be syntactically valid Python.\n");

        Ok(prompt)
    }

    pub(crate) fn build_python_audit_prompt(
        &self,
        context: &EvolutionContext,
        script_content: &str,
    ) -> Result<String> {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "You are a security auditor for Python scripts in the blockcell agent framework.\n\
            Review the following complete Python script for skill '{}'.\n\n",
            context.skill_name
        ));

        prompt.push_str(&format!(
            "Code:\n```python\n{}\n```\n\n",
            Self::sanitize_for_code_fence(script_content)
        ));

        prompt.push_str("\
Check for the following issues:\n\
1. **Syntax errors**: Is this valid Python 3.8+ syntax?\n\
2. **Security**: No shell injection (unsafe os.system/subprocess with user input), no eval/exec of untrusted data\n\
3. **Infinite loops**: Unbounded loops without break conditions\n\
4. **Resource abuse**: Operations that could consume excessive memory or CPU\n\
5. **Data leakage**: Logging/printing sensitive information unintentionally\n\n\
Respond with ONLY a JSON object (no markdown code blocks, no extra text):\n\
{\"passed\": true, \"issues\": []}\n\
or\n\
{\"passed\": false, \"issues\": [{\"severity\": \"error\", \"category\": \"security\", \"message\": \"description\"}]}\n");

        Ok(prompt)
    }

    pub(crate) fn build_local_script_audit_prompt(
        &self,
        context: &EvolutionContext,
        script_content: &str,
    ) -> Result<String> {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "You are a security auditor for local script and CLI assets in the blockcell agent framework.\n\
            Review the following complete script for skill '{}'.\n\n",
            context.skill_name
        ));

        prompt.push_str(&format!(
            "Code:\n```\n{}\n```\n\n",
            Self::sanitize_for_code_fence(script_content)
        ));

        prompt.push_str(
            "Check for the following issues:\n\
1. **Shell injection / command injection**: unsafe string concatenation into commands\n\
2. **Unsafe file access**: path traversal or writing outside the skill directory\n\
3. **Infinite loops**: unbounded loops without break conditions\n\
4. **Resource abuse**: operations that could consume excessive memory or CPU\n\
5. **Data leakage**: logging sensitive information unintentionally\n\n\
Respond with ONLY a JSON object (no markdown code blocks, no extra text):\n\
{\"passed\": true, \"issues\": []}\n\
or\n\
{\"passed\": false, \"issues\": [{\"severity\": \"error\", \"category\": \"security\", \"message\": \"description\"}]}\n",
        );

        Ok(prompt)
    }

    pub(crate) fn append_hybrid_contract_notes(
        &self,
        prompt: &mut String,
        context: &EvolutionContext,
    ) {
        prompt.push_str("## Hybrid Contract\n");
        prompt.push_str("- `SKILL.md` defines the user-facing behavior, the tool flow, and when local execution is appropriate.\n");
        prompt.push_str("- `## Summary {#summary}` is optional and can be used to describe post-execution result polishing for script-backed skills; treat it as a hint, not a hard requirement.\n");
        prompt.push_str(
            "- The file at `source_path` is the executable entrypoint for local behavior.\n",
        );
        prompt.push_str("- Keep the manual and the entrypoint aligned; if you move behavior, update both sides together.\n");
        if let Some(source_path) = context.source_path.as_ref() {
            prompt.push_str(&format!("- Current entrypoint: `{}`\n", source_path));
        }
        prompt.push_str(
            "- Use `exec_local` only for relative paths inside the active skill directory.\n\n",
        );
    }

    pub(crate) fn detect_local_script_syntax_check(
        skill_path: &Path,
    ) -> Option<LocalScriptSyntaxCheck> {
        match skill_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
        {
            "sh" => Some(LocalScriptSyntaxCheck::Shell("sh")),
            "bash" => Some(LocalScriptSyntaxCheck::Shell("bash")),
            "zsh" => Some(LocalScriptSyntaxCheck::Shell("zsh")),
            "js" => Some(LocalScriptSyntaxCheck::Node),
            "php" => Some(LocalScriptSyntaxCheck::Php),
            "rb" => Some(LocalScriptSyntaxCheck::Ruby),
            "py" => Some(LocalScriptSyntaxCheck::Python),
            _ => Self::detect_shebang_syntax_check(skill_path),
        }
    }

    pub(crate) fn detect_shebang_syntax_check(skill_path: &Path) -> Option<LocalScriptSyntaxCheck> {
        let bytes = std::fs::read(skill_path).ok()?;
        let text = std::str::from_utf8(&bytes).ok()?;
        let first_line = text.lines().next()?.trim_start_matches('\u{feff}').trim();
        let shebang = first_line.strip_prefix("#!")?.trim();
        let interpreter = Self::extract_shebang_interpreter(shebang)?;

        match interpreter.as_str() {
            "sh" => Some(LocalScriptSyntaxCheck::Shell("sh")),
            "bash" => Some(LocalScriptSyntaxCheck::Shell("bash")),
            "zsh" => Some(LocalScriptSyntaxCheck::Shell("zsh")),
            "node" => Some(LocalScriptSyntaxCheck::Node),
            "php" => Some(LocalScriptSyntaxCheck::Php),
            "rb" | "ruby" => Some(LocalScriptSyntaxCheck::Ruby),
            "py" | "python" | "python3" => Some(LocalScriptSyntaxCheck::Python),
            _ => None,
        }
    }

    pub(crate) fn extract_shebang_interpreter(shebang: &str) -> Option<String> {
        let mut parts = shebang.split_whitespace();
        let first = parts.next()?;
        let first_name = std::path::Path::new(first)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(first);

        if first_name == "env" {
            let mut next = parts.next()?;
            while next.starts_with('-') {
                next = parts.next()?;
            }
            let next_name = std::path::Path::new(next)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(next);
            return Some(next_name.to_string());
        }

        Some(first_name.to_string())
    }

    pub(crate) fn build_hybrid_audit_prompt(
        &self,
        context: &EvolutionContext,
        script_content: &str,
    ) -> Result<String> {
        let mut prompt = String::new();
        prompt.push_str(
            "You are a security auditor for hybrid skills in the blockcell agent framework.\n",
        );
        prompt.push_str("This skill combines SKILL.md with a local script asset, so audit both the contract and the executable entrypoint.\n\n");

        let body = match context.skill_type {
            SkillType::Python => self.build_python_audit_prompt(context, script_content)?,
            SkillType::LocalScript => {
                self.build_local_script_audit_prompt(context, script_content)?
            }
            SkillType::Rhai => self.build_audit_prompt(context, script_content)?,
            SkillType::PromptOnly => {
                self.build_prompt_only_audit_prompt(context, script_content)?
            }
        };

        prompt.push_str(&body);
        Ok(prompt)
    }

    pub(crate) fn build_python_fix_prompt(
        &self,
        context: &EvolutionContext,
        current_feedback: &FeedbackEntry,
        history: &[FeedbackEntry],
    ) -> Result<String> {
        let is_manual = matches!(context.trigger, TriggerReason::ManualRequest { .. });
        let mut prompt = String::new();

        prompt.push_str("You are a Python skill developer for the blockcell agent framework.\n");
        prompt.push_str("Your task is to fix issues in a SKILL.py Python script.\n\n");

        // Enriched context: project rules, evolution history, adjacent skills
        let enriched = self.gather_evolution_context(context);
        prompt.push_str(&self.format_enriched_context(&enriched));

        if is_manual {
            if let TriggerReason::ManualRequest { ref description } = context.trigger {
                if Self::is_openclaw_import_description(description) {
                    prompt.push_str(&format!("## Original Task\n{}\n\n", description));
                } else {
                    prompt.push_str(&format!(
                        "## Original Task\nCreate or improve a Blockcell SKILL.py for: {}\n\n",
                        description
                    ));
                }
            }
        } else {
            prompt.push_str(&format!(
                "## Original Task\nFix the existing Blockcell SKILL.py for skill '{}'. Keep the same skill scope and repair only the broken parts.\n\n",
                context.skill_name
            ));
        }

        prompt.push_str("## Previous Content (has issues)\n");
        prompt.push_str(&format!(
            "```python\n{}\n```\n\n",
            Self::sanitize_for_code_fence(&current_feedback.previous_code)
        ));

        prompt.push_str(&format!("## Issues Found ({})\n", current_feedback.stage));
        prompt.push_str(&format!("{}\n\n", current_feedback.feedback));

        let prev_attempts: Vec<&FeedbackEntry> = history
            .iter()
            .filter(|h| h.attempt < current_feedback.attempt)
            .collect();
        if !prev_attempts.is_empty() {
            prompt.push_str("## Previous Attempt History\n");
            for entry in prev_attempts {
                prompt.push_str(&format!(
                    "### Attempt #{} ({} failure)\n{}\n\n",
                    entry.attempt, entry.stage, entry.feedback
                ));
            }
        }

        prompt.push_str("## Instructions\n");
        prompt.push_str("Fix ALL the issues listed above and generate the COMPLETE corrected SKILL.py content.\n");
        prompt.push_str("If the skill can directly produce final user-facing text, return `display_text`. Otherwise return `summary_data` as a lightweight structured summary for runtime/LLM polishing. Do NOT place complete webpages, long markdown, or large raw logs into `summary_data`.\n");
        prompt.push_str("Output the Python code in a ```python code block.\n");
        prompt.push_str(
            "If you output `meta.yaml`, it must follow the minimal meta rules below.\n\n",
        );
        prompt.push_str(Self::trigger_rules_prompt());

        Ok(prompt)
    }
}
