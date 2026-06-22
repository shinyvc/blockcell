//! 核心能力进化的「代码生成」阶段。
//!
//! 调用 LLM 生成能力代码、构造生成 prompt、从响应中抽取代码块。
//! 从 `core_evolution.rs` 抽出，作为 `CoreEvolution` 的独立 impl 块。

use blockcell_core::{Error, ProviderKind, Result};
use tracing::{debug, info};

use crate::evolution::LLMProvider;

use super::{CoreEvolution, CoreEvolutionRecord};

impl CoreEvolution {
    /// Generate code for the capability using LLM.
    /// Returns (extracted_code, raw_llm_response) so caller can also extract schemas.
    pub(super) async fn generate_code(
        &self,
        record: &CoreEvolutionRecord,
        llm_provider: &dyn LLMProvider,
    ) -> Result<(String, String)> {
        let prompt = self.build_generation_prompt(record)?;

        debug!(
            evolution_id = %record.id,
            prompt_len = prompt.len(),
            "🧬 [核心进化] 生成 prompt ({} chars)",
            prompt.len()
        );

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(self.llm_timeout_secs),
            llm_provider.generate(&prompt),
        )
        .await
        .map_err(|_| {
            Error::Evolution(format!(
                "LLM call timed out after {} seconds",
                self.llm_timeout_secs
            ))
        })?
        .map_err(|e| Error::Evolution(format!("LLM generation failed: {}", e)))?;
        let code = self.extract_code_from_response(&response, &record.provider_kind)?;

        info!(
            evolution_id = %record.id,
            code_len = code.len(),
            "🧬 [核心进化] 代码已生成 ({} chars)",
            code.len()
        );

        Ok((code, response))
    }

    pub(super) fn build_generation_prompt(&self, record: &CoreEvolutionRecord) -> Result<String> {
        let mut prompt = String::new();

        prompt.push_str(
            "You are a capability evolution engine for the blockcell self-augmenting agent.\n",
        );
        prompt.push_str(
            "Your task is to generate executable code that implements a new capability.\n\n",
        );

        prompt.push_str("## Capability Request\n");
        prompt.push_str(&format!("- **ID**: {}\n", record.capability_id));
        prompt.push_str(&format!("- **Description**: {}\n", record.description));
        prompt.push_str(&format!(
            "- **Provider Type**: {:?}\n\n",
            record.provider_kind
        ));

        match record.provider_kind {
            ProviderKind::Process => {
                prompt.push_str("## Requirements\n");
                prompt.push_str("Generate a shell script that:\n");
                prompt.push_str("1. Reads JSON input from stdin\n");
                prompt.push_str("2. Performs the requested operation\n");
                prompt.push_str("3. Outputs JSON result to stdout\n");
                prompt.push_str("4. Returns exit code 0 on success, non-zero on failure\n\n");
                prompt.push_str(
                    "The script should be self-contained and use only standard system tools.\n",
                );
                prompt.push_str("Use `#!/bin/bash` as the shebang.\n\n");
                prompt.push_str("## Output Format\n");
                prompt.push_str("Output ONLY the shell script in a ```bash code block.\n");
            }
            ProviderKind::ExternalApi => {
                prompt.push_str("## Requirements\n");
                prompt.push_str("Generate a Python script that:\n");
                prompt.push_str(
                    "1. Reads JSON input from the CAPABILITY_INPUT environment variable\n",
                );
                prompt.push_str("2. Performs the requested API call\n");
                prompt.push_str("3. Prints JSON result to stdout\n");
                prompt
                    .push_str("4. Uses only standard library modules (json, urllib, os, sys)\n\n");
                prompt.push_str("## Output Format\n");
                prompt.push_str("Output ONLY the Python script in a ```python code block.\n");
            }
            _ => {
                prompt.push_str("## Requirements\n");
                prompt.push_str("Generate a shell script (bash) that:\n");
                prompt.push_str("1. Reads JSON input from stdin\n");
                prompt.push_str("2. Implements the capability using available system tools\n");
                prompt.push_str("3. Outputs JSON result to stdout\n\n");
                prompt.push_str("## Output Format\n");
                prompt.push_str("Output ONLY the script in a ```bash code block.\n");
            }
        }

        // Request input/output schema alongside the code
        prompt.push_str("\n## Schema Requirement\n");
        prompt.push_str(
            "After the code block, output a JSON block describing the input and output schema.\n",
        );
        prompt.push_str("Format:\n```json\n{\n  \"input_schema\": {\n    \"type\": \"object\",\n    \"properties\": { ... },\n    \"required\": [ ... ]\n  },\n  \"output_schema\": {\n    \"type\": \"object\",\n    \"properties\": { ... }\n  }\n}\n```\n");
        prompt.push_str("This schema helps the agent understand how to call this capability.\n\n");

        // Add feedback history if retrying
        if !record.feedback_history.is_empty() {
            prompt.push_str("\n## Previous Attempts (FAILED — fix these issues)\n");
            for entry in &record.feedback_history {
                prompt.push_str(&format!(
                    "### Attempt #{} ({} failure)\n",
                    entry.attempt, entry.stage
                ));
                prompt.push_str(&format!("**Issue**: {}\n", entry.feedback));
                prompt.push_str(&format!(
                    "**Previous code**:\n```\n{}\n```\n\n",
                    entry.previous_code
                ));
            }
            prompt.push_str("Fix ALL the issues above. Do NOT repeat the same mistakes.\n");
        }

        Ok(prompt)
    }

    pub(super) fn extract_code_from_response(
        &self,
        response: &str,
        provider_kind: &ProviderKind,
    ) -> Result<String> {
        // Try language-specific code blocks first
        let markers = match provider_kind {
            ProviderKind::Process | ProviderKind::BuiltIn => vec!["```bash", "```sh", "```shell"],
            ProviderKind::ExternalApi => vec!["```python", "```py"],
            _ => vec!["```bash", "```sh", "```python"],
        };

        for marker in &markers {
            if let Some(start) = response.find(marker) {
                let after = start + marker.len();
                if let Some(end) = response[after..].find("```") {
                    return Ok(response[after..after + end].trim().to_string());
                }
            }
        }

        // Fallback: generic code block
        if let Some(start) = response.find("```") {
            let after = start + 3;
            let content_start = response[after..]
                .find('\n')
                .map(|i| after + i + 1)
                .unwrap_or(after);
            if let Some(end) = response[content_start..].find("```") {
                return Ok(response[content_start..content_start + end]
                    .trim()
                    .to_string());
            }
        }

        // Last resort: entire response
        Ok(response.trim().to_string())
    }
}
