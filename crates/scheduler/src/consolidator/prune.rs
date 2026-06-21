use super::*;

impl DreamConsolidator {
    /// 阶段 4: 修剪索引
    pub(crate) async fn prune(&self) -> Result<DreamStats, DreamError> {
        tracing::debug!("[dream] Phase 4: Pruning indexes");

        // 清理过期的 session memory 文件
        self.prune_expired_session_memories().await
    }

    /// 清理过期的 session memory 文件
    pub(crate) async fn prune_expired_session_memories(&self) -> Result<DreamStats, DreamError> {
        let sessions_dir = self.config_dir.join("sessions");

        if !fs::try_exists(&sessions_dir).await? {
            return Ok(DreamStats::default());
        }

        let expiry_threshold = SESSION_MEMORY_EXPIRY_DAYS * 24 * 3600; // 转换为秒
        let active_threshold = 3600; // 1小时内更新视为活跃会话
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut entries = fs::read_dir(&sessions_dir).await?;
        let mut pruned_count = 0;
        let mut skipped_active = 0;

        while let Some(entry) = entries.next_entry().await? {
            // 跳过非目录条目（如 .DS_Store）
            if entry.file_type().await.map(|t| !t.is_dir()).unwrap_or(true) {
                continue;
            }
            let session_dir = entry.path();

            // 检查是否为活跃会话
            if self
                .is_session_active(&session_dir, now, active_threshold)
                .await?
            {
                skipped_active += 1;
                continue;
            }

            // 检查目录修改时间
            if let Ok(metadata) = fs::metadata(&session_dir).await {
                if let Ok(modified) = metadata.modified() {
                    let modified_secs = modified
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    // 如果超过过期阈值，删除整个目录
                    if now - modified_secs > expiry_threshold {
                        tracing::trace!(
                            path = %session_dir.display(),
                            age_days = (now - modified_secs) / (24 * 3600),
                            "pruning expired session memory"
                        );
                        fs::remove_dir_all(&session_dir).await?;
                        pruned_count += 1;
                    }
                }
            }
        }

        tracing::info!(
            pruned_count,
            skipped_active,
            "[dream] Phase 4: Pruned {} expired session memories ({} active sessions skipped)",
            pruned_count,
            skipped_active
        );

        Ok(DreamStats {
            sessions_pruned: pruned_count,
            sessions_processed: pruned_count + skipped_active,
            ..Default::default()
        })
    }

    /// 检查会话是否仍在活跃运行
    ///
    /// 通过检查 `.active` 文件是否存在且最近更新来判断。
    /// 如果文件不存在或超过阈值时间未更新，则视为非活跃。
    pub(crate) async fn is_session_active(
        &self,
        session_dir: &Path,
        now: u64,
        active_threshold_secs: u64,
    ) -> Result<bool, DreamError> {
        let active_file = session_dir.join(".active");

        // 如果 .active 文件不存在，会话非活跃
        if !fs::try_exists(&active_file).await? {
            return Ok(false);
        }

        // 检查文件修改时间
        match fs::metadata(&active_file).await {
            Ok(metadata) => {
                match metadata.modified() {
                    Ok(modified) => {
                        let modified_secs = modified
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);

                        // 如果最近有更新，视为活跃
                        let is_active = now.saturating_sub(modified_secs) < active_threshold_secs;
                        Ok(is_active)
                    }
                    Err(_) => Ok(false),
                }
            }
            Err(_) => Ok(false),
        }
    }

    /// 增加会话计数
    pub fn increment_session_count(&mut self) {
        self.state.increment_session_count();
    }

    /// 获取当前状态
    pub fn state(&self) -> &DreamState {
        &self.state
    }
}
