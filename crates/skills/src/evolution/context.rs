use super::*;

impl SkillEvolution {
    /// Gather enriched context for evolution prompts.
    /// Reads BLOCKCELL.md (project-level rules), SKILL.md, manual/evolution.md,
    /// and adjacent skills of the same type.
    pub(crate) fn gather_evolution_context(
        &self,
        context: &EvolutionContext,
    ) -> EnrichedEvolutionContext {
        let skills_dir = self.skill_root_dir_by_name(
            &context.skill_name,
            context.staged,
            context.staging_skills_dir.as_deref(),
        );
        let skill_dir = skills_dir.join(&context.skill_name);

        // 1. Read BLOCKCELL.md — walk up from skills_dir to find it
        let blockcell_md = self.find_and_read_blockcell_md(&skills_dir);

        // 2. Read SKILL.md (the runtime contract)
        let skill_md = std::fs::read_to_string(skill_dir.join("SKILL.md")).ok();

        // 3. Read manual/evolution.md (historical fix experience)
        let evolution_history_md =
            std::fs::read_to_string(skill_dir.join("manual").join("evolution.md")).ok();

        // 4. Find adjacent skills of the same type (max 3, max 500 chars each)
        let adjacent_skills = self.find_adjacent_skills(&context.skill_name, &context.layout);

        // 5. Collect recent completed evolution records for this skill
        let recent_evolutions = self.load_recent_evolution_summaries(&context.skill_name, 3);

        EnrichedEvolutionContext {
            blockcell_md,
            skill_md,
            evolution_history_md,
            adjacent_skills,
            recent_evolutions,
        }
    }

    /// Walk up from skills_dir to find BLOCKCELL.md (or CLAUDE.md as fallback)
    pub(crate) fn find_and_read_blockcell_md(&self, skills_dir: &Path) -> Option<String> {
        let mut dir = skills_dir.to_path_buf();
        // Walk up at most 4 levels (skills -> workspace -> .blockcell -> home)
        for _ in 0..4 {
            let candidate = dir.join("BLOCKCELL.md");
            if candidate.exists() {
                if let Ok(content) = std::fs::read_to_string(&candidate) {
                    let truncated: String = content.chars().take(2000).collect();
                    return Some(truncated);
                }
            }
            // Also check CLAUDE.md as fallback
            let claude_candidate = dir.join("CLAUDE.md");
            if claude_candidate.exists() {
                if let Ok(content) = std::fs::read_to_string(&claude_candidate) {
                    let truncated: String = content.chars().take(2000).collect();
                    return Some(truncated);
                }
            }
            if !dir.pop() {
                break;
            }
        }
        None
    }

    /// Find adjacent skills of the same SkillLayout, return up to `max` snippet references.
    pub(crate) fn find_adjacent_skills(
        &self,
        skill_name: &str,
        layout: &SkillLayout,
    ) -> Vec<AdjacentSkillRef> {
        let mut refs = Vec::new();
        let skills_dir = &self.skills_dir;

        let entries = match std::fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(_) => return refs,
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == skill_name {
                continue;
            }
            if !entry.path().is_dir() {
                continue;
            }

            // Detect layout of this adjacent skill
            let adj_layout = self.detect_adjacent_skill_layout(&name);
            if &adj_layout != layout {
                continue;
            }

            // Read SKILL.md snippet
            let skill_md_path = entry.path().join("SKILL.md");
            if let Ok(content) = std::fs::read_to_string(&skill_md_path) {
                if !content.trim().is_empty() {
                    refs.push(AdjacentSkillRef {
                        name,
                        snippet: content.chars().take(500).collect(),
                    });
                }
            }

            if refs.len() >= 3 {
                break;
            }
        }
        refs
    }

    /// Simple skill layout detection for adjacent skills (no truncation needed)
    pub(crate) fn detect_adjacent_skill_layout(&self, skill_name: &str) -> SkillLayout {
        let skill_dir = self.skills_dir.join(skill_name);
        let has_md = skill_dir.join("SKILL.md").exists();
        if skill_dir.join("SKILL.rhai").exists() {
            SkillLayout::RhaiOrchestration
        } else if skill_dir.join("SKILL.py").exists()
            || Self::contains_local_script_asset(&skill_dir)
        {
            if has_md {
                SkillLayout::Hybrid
            } else {
                SkillLayout::LocalScript
            }
        } else {
            SkillLayout::PromptTool
        }
    }

    pub(crate) fn contains_local_script_asset(skill_dir: &Path) -> bool {
        let script_dir = skill_dir.join("scripts");
        let bin_dir = skill_dir.join("bin");

        if Self::dir_contains_local_script(&script_dir) || Self::dir_contains_local_script(&bin_dir)
        {
            return true;
        }

        let Ok(entries) = std::fs::read_dir(skill_dir) else {
            return false;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if matches!(
                file_name,
                "SKILL.md" | "SKILL.rhai" | "meta.yaml" | "meta.json"
            ) {
                continue;
            }

            let ext_ok = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext, "py" | "sh" | "php" | "js" | "ts" | "rb"));
            let no_ext_exec = path.extension().is_none() && Self::looks_executable(&path);
            if ext_ok || no_ext_exec {
                return true;
            }
        }

        false
    }

    pub(crate) fn dir_contains_local_script(dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if Self::dir_contains_local_script(&path) {
                    return true;
                }
                continue;
            }

            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext, "py" | "sh" | "php" | "js" | "ts" | "rb"))
            {
                return true;
            }
        }

        false
    }

    #[cfg(unix)]
    pub(crate) fn looks_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;

        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    pub(crate) fn looks_executable(_path: &Path) -> bool {
        false
    }

    /// Load recent completed/failed evolution summaries for a skill (for prompt injection)
    pub(crate) fn load_recent_evolution_summaries(
        &self,
        skill_name: &str,
        max: usize,
    ) -> Vec<String> {
        let records_dir = self.records_dir();
        if !records_dir.exists() {
            return Vec::new();
        }

        let mut summaries: Vec<(i64, String)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&records_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "json") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(record) = serde_json::from_str::<EvolutionRecord>(&content) {
                        if record.skill_name != skill_name {
                            continue;
                        }
                        if !matches!(
                            record.status,
                            EvolutionStatus::Completed
                                | EvolutionStatus::RolledBack
                                | EvolutionStatus::Failed
                        ) {
                            continue;
                        }
                        let summary = format!(
                            "[{:?}] attempt={}, trigger={:?}{}",
                            record.status,
                            record.attempt,
                            record.context.trigger,
                            record
                                .patch
                                .as_ref()
                                .map(|p| {
                                    let expl: String = p.explanation.chars().take(150).collect();
                                    format!(", explanation={}", expl)
                                })
                                .unwrap_or_default()
                        );
                        summaries.push((record.created_at, summary));
                    }
                }
            }
        }

        summaries.sort_by_key(|b| std::cmp::Reverse(b.0));
        summaries.into_iter().take(max).map(|(_, s)| s).collect()
    }

    /// Helper: resolve skill root dir by name (handles staged vs normal)
    /// Applies the same path traversal validation as `skill_root_dir_for_record`.
    pub(crate) fn skill_root_dir_by_name(
        &self,
        _skill_name: &str,
        staged: bool,
        staging_dir: Option<&str>,
    ) -> PathBuf {
        if staged {
            if let Some(dir) = staging_dir {
                let p = PathBuf::from(dir);
                if p.is_absolute() {
                    // Validate: staging dir must be within the workspace directory tree
                    // to prevent path traversal attacks (same logic as skill_root_dir_for_record).
                    if let Ok(canonical_staging) = p.canonicalize() {
                        if let Ok(canonical_skills) = self.skills_dir.canonicalize() {
                            let canonical_workspace = canonical_skills.parent();
                            if let Some(workspace) = canonical_workspace {
                                if canonical_staging.starts_with(workspace) {
                                    return p;
                                }
                            }
                            if canonical_staging.starts_with(&canonical_skills) {
                                return p;
                            }
                        }
                    }
                    warn!(
                        path = %dir,
                        "skill_root_dir_for_record: staging_skills_dir is outside workspace directory tree, ignoring"
                    );
                    warn!(
                        path = %dir,
                        "skill_root_dir_by_name: staging_skills_dir is outside workspace directory tree, ignoring"
                    );
                }
            }
        }
        self.skills_dir.clone()
    }

    /// Format enriched context as prompt sections
    pub(crate) fn format_enriched_context(&self, enriched: &EnrichedEvolutionContext) -> String {
        let mut sections = String::new();

        if let Some(ref md) = enriched.blockcell_md {
            sections.push_str("## Project Rules (BLOCKCELL.md)\n");
            sections.push_str(md);
            sections.push_str("\n\n");
        }

        if let Some(ref md) = enriched.skill_md {
            let truncated: String = md.chars().take(1500).collect();
            sections.push_str("## Current SKILL.md (Runtime Contract)\n");
            sections.push_str(&truncated);
            sections.push_str("\n\n");
        }

        if let Some(ref md) = enriched.evolution_history_md {
            let truncated: String = md.chars().take(1000).collect();
            sections.push_str("## Historical Fix Experience (manual/evolution.md)\n");
            sections.push_str(&truncated);
            sections.push_str("\n\n");
        }

        if !enriched.adjacent_skills.is_empty() {
            sections
                .push_str("## Adjacent Skills Reference (same layout, for style consistency)\n");
            for adj in &enriched.adjacent_skills {
                sections.push_str(&format!("### {}\n{}\n\n", adj.name, adj.snippet));
            }
        }

        if !enriched.recent_evolutions.is_empty() {
            sections.push_str("## Recent Evolution History (avoid repeating past failures)\n");
            for summary in &enriched.recent_evolutions {
                sections.push_str(&format!("- {}\n", summary));
            }
            sections.push('\n');
        }

        sections
    }
}
