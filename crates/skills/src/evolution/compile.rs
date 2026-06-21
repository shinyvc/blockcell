use super::*;

impl SkillEvolution {
    pub(crate) async fn compile_local_script(
        &self,
        skill_path: &Path,
    ) -> Result<(bool, Option<String>)> {
        let syntax_check = Self::detect_local_script_syntax_check(skill_path);

        if let Some(check) = syntax_check {
            let output = check.run(skill_path);

            match output {
                Ok(out) if out.status.success() => return Ok((true, None)),
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let message = if !stderr.trim().is_empty() {
                        stderr
                    } else if !stdout.trim().is_empty() {
                        stdout
                    } else {
                        format!(
                            "Local script syntax check failed for {:?}",
                            skill_path.file_name()
                        )
                    };
                    return Ok((false, Some(message)));
                }
                Err(e) => {
                    // Syntax checker unavailable — treat as failure rather than
                    // silently passing an unverified script.
                    return Ok((
                        false,
                        Some(format!(
                            "Syntax checker unavailable or failed to run: {}",
                            e
                        )),
                    ));
                }
            }
        }

        let content = std::fs::read(skill_path)
            .map_err(|e| Error::Skill(format!("Failed to read local script: {}", e)))?;

        if content.is_empty() {
            return Ok((false, Some("Local script content is empty".to_string())));
        }

        Ok((
            true,
            Some(if skill_path.extension().is_none() {
                "No extension or recognized shebang detected; skipped syntax-specific validation"
                    .to_string()
            } else {
                "Skipped syntax-specific validation for unsupported script type".to_string()
            }),
        ))
    }

    pub(crate) fn extract_diff_from_response(&self, response: &str) -> Result<String> {
        // Try ```diff block (for patching existing skills).
        // Use rfind to get the LAST occurrence, which is typically the actual
        // code block in LLM output (earlier ones may be in explanation text).
        if let Some(start) = response.rfind("```diff") {
            let after_marker = start + 7;
            // Find the first closing ``` after the opening marker
            if let Some(end) = response[after_marker..].find("```") {
                let diff = &response[after_marker..after_marker + end];
                return Ok(diff.trim().to_string());
            }
        }

        // Try ```rhai block (for new skill creation — full script output)
        if let Some(start) = response.rfind("```rhai") {
            let after_marker = start + 7;
            if let Some(end) = response[after_marker..].find("```") {
                let script = &response[after_marker..after_marker + end];
                return Ok(script.trim().to_string());
            }
        }

        // Try ```python block (for Python skill creation)
        if let Some(start) = response.rfind("```python") {
            let after_marker = start + 9;
            if let Some(end) = response[after_marker..].find("```") {
                let script = &response[after_marker..after_marker + end];
                return Ok(script.trim().to_string());
            }
        }

        // Try ```markdown block (for prompt-only skills)
        if let Some(start) = response.rfind("```markdown") {
            let after_marker = start + 11;
            if let Some(end) = response[after_marker..].find("```") {
                let md = &response[after_marker..after_marker + end];
                return Ok(md.trim().to_string());
            }
        }

        // Try generic ``` block
        if let Some(start) = response.rfind("```") {
            let after_marker = start + 3;
            let content_start = response[after_marker..]
                .find('\n')
                .map(|i| after_marker + i + 1)
                .unwrap_or(after_marker);
            if let Some(end) = response[content_start..].find("```") {
                let content = &response[content_start..content_start + end];
                return Ok(content.trim().to_string());
            }
        }

        // Fallback: entire response
        Ok(response.trim().to_string())
    }

    pub(crate) fn parse_audit_response(&self, response: &str) -> Result<AuditResult> {
        // Extract JSON from ```json code blocks if present
        let json_str = if let Some(start) = response.find("```json") {
            let after_marker = start + 7;
            if let Some(end) = response[after_marker..].find("```") {
                response[after_marker..after_marker + end].trim()
            } else {
                response.trim()
            }
        } else if let Some(start) = response.find("```") {
            let after_marker = start + 3;
            // Skip optional language tag on same line
            let content_start = response[after_marker..]
                .find('\n')
                .map(|i| after_marker + i + 1)
                .unwrap_or(after_marker);
            if let Some(end) = response[content_start..].find("```") {
                response[content_start..content_start + end].trim()
            } else {
                response.trim()
            }
        } else {
            response.trim()
        };

        let parsed: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| Error::Evolution(format!("Failed to parse audit response: {}", e)))?;

        let passed = parsed["passed"].as_bool().unwrap_or(false);
        let empty_vec = vec![];
        let issues_json = parsed["issues"].as_array().unwrap_or(&empty_vec);

        let issues = issues_json
            .iter()
            .filter_map(|i| {
                Some(AuditIssue {
                    severity: i["severity"].as_str()?.to_string(),
                    category: i["category"].as_str()?.to_string(),
                    message: i["message"].as_str()?.to_string(),
                })
            })
            .collect();

        Ok(AuditResult {
            passed,
            issues,
            audited_at: chrono::Utc::now().timestamp(),
        })
    }

    /// 解析最终脚本内容
    ///
    /// P0-2: 由于所有生成都输出完整脚本，这里直接返回 patch.diff 内容。
    /// 保留此方法作为统一入口，便于未来扩展。
    pub(crate) fn resolve_final_script(
        &self,
        _skill_name: &str,
        script_content: &str,
    ) -> Result<String> {
        Ok(script_content.to_string())
    }

    /// 编译 Rhai 脚本，返回 (是否成功, 错误信息)
    pub(crate) async fn compile_skill(&self, skill_path: &Path) -> Result<(bool, Option<String>)> {
        let engine = rhai::Engine::new();
        let content = std::fs::read_to_string(skill_path)?;

        match engine.compile(&content) {
            Ok(_ast) => {
                info!("🔨 [compile] Rhai compilation succeeded");
                Ok((true, None))
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                warn!(
                    error = %e,
                    "🔨 [compile] Rhai compilation FAILED: {}",
                    e
                );
                Ok((false, Some(error_msg)))
            }
        }
    }

    pub(crate) async fn compile_rhai_check(
        &self,
        evolution_id: &str,
        skill_name: &str,
        script_content: &str,
    ) -> Result<(bool, Option<String>)> {
        let temp_guard = TempFileGuard::new(std::env::temp_dir().join(format!(
            "{}_compile_{}.rhai",
            skill_name,
            RECORD_TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        )));
        std::fs::write(temp_guard.path(), script_content)?;

        info!(
            evolution_id = %evolution_id,
            content_len = script_content.len(),
            content_lines = script_content.lines().count(),
            "🔨 [compile] Script: {} chars, {} lines",
            script_content.len(),
            script_content.lines().count()
        );
        debug!(
            evolution_id = %evolution_id,
            "🔨 [compile] Script content:\n{}",
            script_content
        );

        info!(evolution_id = %evolution_id, "🔨 [compile] Compiling with Rhai engine...");
        let result = self.compile_skill(temp_guard.path()).await;
        // TempFileGuard's Drop handles cleanup even on panic/cancellation
        result
    }
}
