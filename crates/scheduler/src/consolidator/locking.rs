use super::*;

impl DreamConsolidator {
    /// 获取锁
    ///
    /// 使用原子 rename 操作避免 TOCTOU 竞争条件。
    /// 锁文件格式: `PID:TIMESTAMP`
    ///
    /// ## 算法
    /// 1. 先创建临时锁文件（带唯一标识）
    /// 2. 检查现有锁是否过期
    /// 3. 如果过期，尝试原子 rename（只有一个进程会成功）
    /// 4. 如果 rename 失败，说明另一个进程已获取锁
    pub(crate) async fn acquire_lock(&self) -> Result<(), DreamError> {
        use std::process;

        let lock_path = self.config_dir.join(LOCK_FILE_NAME);
        let temp_lock_path =
            self.config_dir
                .join(format!("{}.tmp.{}", LOCK_FILE_NAME, process::id()));
        let current_pid = process::id();
        let max_retries = 3;

        for attempt in 0..max_retries {
            // 1. 先创建临时锁文件（每个进程有自己的临时文件，无竞争）
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let lock_content = format!("{}:{}", current_pid, timestamp);

            // 确保配置目录存在
            if let Some(parent) = lock_path.parent() {
                fs::create_dir_all(parent).await?;
            }

            // 写入临时文件
            fs::write(&temp_lock_path, &lock_content).await?;

            // 2. 检查现有锁是否存在且有效
            match fs::try_exists(&lock_path).await {
                Ok(true) => {
                    // 锁文件存在，检查是否过期
                    match check_lock_validity(&lock_path).await {
                        Ok(true) => {
                            // 锁仍然有效，清理临时文件并返回
                            tracing::debug!(attempt, "[dream] Lock is held by another process");
                            let _ = fs::remove_file(&temp_lock_path).await;
                            return Err(DreamError::LockAcquired);
                        }
                        Ok(false) => {
                            // 锁已过期，尝试原子替换
                            // rename 在大多数平台上是原子的
                            match fs::rename(&temp_lock_path, &lock_path).await {
                                Ok(()) => {
                                    tracing::debug!(
                                        pid = current_pid,
                                        attempt,
                                        "[dream] Lock acquired (replaced stale lock)"
                                    );
                                    return Ok(());
                                }
                                Err(e) => {
                                    // rename 失败，可能另一个进程已获取锁
                                    tracing::warn!(
                                        error = %e,
                                        attempt,
                                        "[dream] Failed to replace stale lock, retrying"
                                    );
                                    let _ = fs::remove_file(&temp_lock_path).await;
                                    // 继续重试
                                }
                            }
                        }
                        Err(e) => {
                            // 无法读取锁文件，尝试替换
                            tracing::warn!(
                                error = %e,
                                "[dream] Cannot read lock file, attempting to replace"
                            );
                            match fs::rename(&temp_lock_path, &lock_path).await {
                                Ok(()) => {
                                    tracing::debug!(
                                        pid = current_pid,
                                        "[dream] Lock acquired (replaced corrupted lock)"
                                    );
                                    return Ok(());
                                }
                                Err(_e) => {
                                    let _ = fs::remove_file(&temp_lock_path).await;
                                }
                            }
                        }
                    }
                }
                Ok(false) => {
                    // 锁文件不存在，尝试创建
                    match fs::rename(&temp_lock_path, &lock_path).await {
                        Ok(()) => {
                            tracing::debug!(
                                pid = current_pid,
                                attempt,
                                "[dream] Lock acquired (new lock)"
                            );
                            return Ok(());
                        }
                        Err(e) => {
                            // rename 失败（可能另一个进程同时创建）
                            tracing::warn!(
                                error = %e,
                                attempt,
                                "[dream] Failed to create lock, retrying"
                            );
                            let _ = fs::remove_file(&temp_lock_path).await;
                            // 继续重试
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "[dream] Cannot check lock existence"
                    );
                    let _ = fs::remove_file(&temp_lock_path).await;
                    return Err(e.into());
                }
            }
        }

        // 重试次数耗尽
        tracing::error!(
            attempts = max_retries,
            "[dream] Failed to acquire lock after max retries"
        );
        // 清理临时文件
        let _ = fs::remove_file(&temp_lock_path).await;
        Err(DreamError::LockAcquired)
    }

    /// 释放锁
    pub(crate) async fn release_lock(&self) -> Result<(), DreamError> {
        let lock_path = self.config_dir.join(LOCK_FILE_NAME);
        if fs::try_exists(&lock_path).await? {
            fs::remove_file(&lock_path).await?;
        }
        Ok(())
    }

    /// 阶段 1: 定位现有内容
    pub(crate) async fn orient(&self) -> Result<(), DreamError> {
        tracing::debug!("[dream] Phase 1: Orienting");

        // 读取现有记忆文件，建立索引
        let memory_dir = self.config_dir.join("memory");
        if !fs::try_exists(&memory_dir).await? {
            fs::create_dir_all(&memory_dir).await?;
        }

        Ok(())
    }
}
