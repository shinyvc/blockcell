use super::*;

impl SkillEvolution {
    /// 审计补丁（独立 LLM 会话）
    ///
    /// P0-1 fix: 审计基于应用后的完整脚本，而非原始 patch.diff
    pub async fn audit_patch(
        &self,
        evolution_id: &str,
        llm_provider: &dyn LLMProvider,
    ) -> Result<AuditResult> {
        let mut record = self.load_record(evolution_id)?;

        // Precondition: must be in Generated state (or Auditing for retry)
        if !matches!(
            record.status,
            EvolutionStatus::Generated | EvolutionStatus::Auditing
        ) {
            return Err(Error::Evolution(format!(
                "Cannot audit patch: expected status Generated, got {:?}",
                record.status
            )));
        }

        record.status = EvolutionStatus::Auditing;
        self.save_record(&record)?;

        let patch = record
            .patch
            .as_ref()
            .ok_or_else(|| Error::Evolution("No patch to audit".to_string()))?;

        info!(evolution_id = %evolution_id, "Auditing patch");

        // P0-1: 解析最终脚本内容用于审计（而非 diff 文本）
        let final_script = self.resolve_final_script(&record.skill_name, &patch.diff)?;

        let prompt = match record.context.layout {
            SkillLayout::PromptTool => {
                self.build_prompt_only_audit_prompt(&record.context, &final_script)?
            }
            SkillLayout::LocalScript => {
                self.build_local_script_audit_prompt(&record.context, &final_script)?
            }
            SkillLayout::Hybrid => {
                self.build_hybrid_audit_prompt(&record.context, &final_script)?
            }
            SkillLayout::RhaiOrchestration => {
                self.build_audit_prompt(&record.context, &final_script)?
            }
        };

        info!(
            evolution_id = %evolution_id,
            prompt_len = prompt.len(),
            "🔍 [audit] Audit prompt built"
        );
        debug!(
            evolution_id = %evolution_id,
            "🔍 [audit] Full audit prompt:\n{}",
            prompt
        );

        info!(evolution_id = %evolution_id, "🔍 [audit] Calling LLM...");
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

        info!(
            evolution_id = %evolution_id,
            response_len = response.len(),
            "🔍 [audit] LLM response received"
        );
        debug!(
            evolution_id = %evolution_id,
            "🔍 [audit] Full LLM response:\n{}",
            response
        );

        let audit_result = self.parse_audit_response(&response)?;

        info!(
            evolution_id = %evolution_id,
            passed = audit_result.passed,
            issues_count = audit_result.issues.len(),
            "🔍 [audit] Audit result: passed={}, issues={}",
            audit_result.passed, audit_result.issues.len()
        );
        for (i, issue) in audit_result.issues.iter().enumerate() {
            info!(
                evolution_id = %evolution_id,
                "🔍 [audit]   Issue #{}: [{}][{}] {}",
                i + 1, issue.severity, issue.category, issue.message
            );
        }

        record.audit = Some(audit_result.clone());
        let new_status = if audit_result.passed {
            EvolutionStatus::AuditPassed
        } else {
            EvolutionStatus::AuditFailed
        };
        info!(
            evolution_id = %evolution_id,
            "🔍 [audit] Status -> {:?}",
            new_status
        );
        record.status = new_status;
        record.updated_at = chrono::Utc::now().timestamp();
        self.save_record(&record)?;

        Ok(audit_result)
    }
}
