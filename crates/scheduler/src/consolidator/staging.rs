use super::*;

pub(crate) fn fingerprint_bytes(bytes: &[u8]) -> FileFingerprint {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    FileFingerprint {
        len: bytes.len() as u64,
        hash: hasher.finish(),
    }
}

pub(crate) fn is_markdown_memory_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
}

pub(crate) fn should_commit_staged_file(rel_path: &Path) -> bool {
    if rel_path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return false;
    }

    if rel_path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|part| part.starts_with(".dream_") || part.ends_with(".tmp"))
    }) {
        return false;
    }

    is_markdown_memory_file(rel_path)
}

pub(crate) fn collect_memory_files_sync(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    if !current.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::other(format!(
                "symlink is not allowed in dream memory staging: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            collect_memory_files_sync(root, &path, files)?;
        } else if metadata.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            files.push(rel.to_path_buf());
        }
    }

    Ok(())
}

pub(crate) async fn snapshot_memory_tree(root: &Path) -> Result<MemoryTreeSnapshot, DreamError> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<MemoryTreeSnapshot, std::io::Error> {
        let mut rel_files = Vec::new();
        collect_memory_files_sync(&root, &root, &mut rel_files)?;

        let mut files = HashMap::new();
        for rel_path in rel_files {
            let path = root.join(&rel_path);
            let bytes = std::fs::read(&path)?;
            files.insert(rel_path, fingerprint_bytes(&bytes));
        }

        Ok(MemoryTreeSnapshot { files })
    })
    .await
    .map_err(|e| DreamError::Io(std::io::Error::other(e.to_string())))?
    .map_err(DreamError::Io)
}

pub(crate) fn copy_memory_tree_sync(source: &Path, dest: &Path) -> std::io::Result<()> {
    if !source.exists() {
        std::fs::create_dir_all(dest)?;
        return Ok(());
    }

    let mut rel_files = Vec::new();
    collect_memory_files_sync(source, source, &mut rel_files)?;
    for rel_path in rel_files {
        let source_path = source.join(&rel_path);
        let dest_path = dest.join(&rel_path);
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source_path, dest_path)?;
    }
    Ok(())
}

pub(crate) async fn prepare_dream_staging(
    config_dir: &Path,
    real_memory_dir: &Path,
) -> Result<DreamStagingRun, DreamError> {
    let root = config_dir
        .join(".dream_staging")
        .join(format!("run_{}", uuid::Uuid::new_v4().simple()));
    let memory_dir = root.join("memory");
    let real_memory_dir = real_memory_dir.to_path_buf();
    let memory_dir_for_copy = memory_dir.clone();

    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        std::fs::create_dir_all(&memory_dir_for_copy)?;
        copy_memory_tree_sync(&real_memory_dir, &memory_dir_for_copy)
    })
    .await
    .map_err(|e| DreamError::Io(std::io::Error::other(e.to_string())))?
    .map_err(DreamError::Io)?;

    Ok(DreamStagingRun {
        root: memory_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| config_dir.join(".dream_staging")),
        memory_dir,
    })
}

pub(crate) async fn validate_staged_memory(staging_memory_dir: &Path) -> Result<(), DreamError> {
    let staging_memory_dir = staging_memory_dir.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
        let mut rel_files = Vec::new();
        collect_memory_files_sync(&staging_memory_dir, &staging_memory_dir, &mut rel_files)?;
        Ok(())
    })
    .await
    .map_err(|e| DreamError::Io(std::io::Error::other(e.to_string())))?
    .map_err(DreamError::Io)
}

pub(crate) fn changed_staged_paths<'a>(
    pre: &'a MemoryTreeSnapshot,
    post: &'a MemoryTreeSnapshot,
) -> impl Iterator<Item = &'a PathBuf> {
    post.files
        .iter()
        .filter_map(|(rel_path, post_fingerprint)| {
            (pre.files.get(rel_path) != Some(post_fingerprint)).then_some(rel_path)
        })
}

pub(crate) fn deleted_staged_paths<'a>(
    pre: &'a MemoryTreeSnapshot,
    post: &'a MemoryTreeSnapshot,
) -> impl Iterator<Item = &'a PathBuf> {
    pre.files
        .keys()
        .filter(|rel_path| !post.files.contains_key(*rel_path))
}

#[cfg(test)]
pub(crate) async fn commit_staged_memory(
    real_memory_dir: &Path,
    pre: &MemoryTreeSnapshot,
    staging_memory_dir: &Path,
) -> Result<DreamStats, DreamError> {
    let commit = commit_staged_memory_transaction(real_memory_dir, pre, staging_memory_dir).await?;
    let stats = commit.stats.clone();
    commit.finalize().await;
    Ok(stats)
}

pub(crate) async fn commit_staged_memory_transaction(
    real_memory_dir: &Path,
    pre: &MemoryTreeSnapshot,
    staging_memory_dir: &Path,
) -> Result<StagedMemoryCommit, DreamError> {
    commit_staged_memory_inner(real_memory_dir, pre, staging_memory_dir, None).await
}

#[cfg(test)]
pub(crate) async fn commit_staged_memory_with_injected_write_failure_for_test(
    real_memory_dir: &Path,
    pre: &MemoryTreeSnapshot,
    staging_memory_dir: &Path,
    fail_rel_path: &Path,
) -> Result<DreamStats, DreamError> {
    let commit = commit_staged_memory_inner(
        real_memory_dir,
        pre,
        staging_memory_dir,
        Some(fail_rel_path),
    )
    .await?;
    let stats = commit.stats.clone();
    commit.finalize().await;
    Ok(stats)
}

pub(crate) async fn commit_staged_memory_inner(
    real_memory_dir: &Path,
    pre: &MemoryTreeSnapshot,
    staging_memory_dir: &Path,
    fail_rel_path: Option<&Path>,
) -> Result<StagedMemoryCommit, DreamError> {
    validate_staged_memory(staging_memory_dir).await?;
    let post = snapshot_memory_tree(staging_memory_dir).await?;
    let current = snapshot_memory_tree(real_memory_dir).await?;

    let mut changes = Vec::new();
    let mut candidate_paths: HashSet<PathBuf> = HashSet::new();
    for rel_path in changed_staged_paths(pre, &post) {
        if !should_commit_staged_file(rel_path) {
            return Err(DreamError::ConsolidationFailed(format!(
                "unsupported file changed in dream staging: {}",
                rel_path.display()
            )));
        }
    }
    for rel_path in deleted_staged_paths(pre, &post) {
        if !should_commit_staged_file(rel_path) {
            return Err(DreamError::ConsolidationFailed(format!(
                "unsupported file deleted in dream staging: {}",
                rel_path.display()
            )));
        }
    }

    for rel_path in changed_staged_paths(pre, &post) {
        if should_commit_staged_file(rel_path) {
            candidate_paths.insert(rel_path.clone());
        }
    }

    for rel_path in pre.files.keys() {
        if should_commit_staged_file(rel_path) && !post.files.contains_key(rel_path) {
            return Err(DreamError::ConsolidationFailed(format!(
                "Dream staging attempted to delete memory file {}; deletion is not enabled",
                rel_path.display()
            )));
        }
    }

    for rel_path in candidate_paths {
        let post_fingerprint = post.files.get(&rel_path);
        let pre_fingerprint = pre.files.get(&rel_path);
        if post_fingerprint == pre_fingerprint {
            continue;
        }

        if current.files.get(&rel_path) != pre_fingerprint {
            return Err(DreamError::ConsolidationFailed(format!(
                "memory file {} changed during dream staging",
                rel_path.display()
            )));
        }

        changes.push(StagedMemoryChange {
            staged_path: staging_memory_dir.join(&rel_path),
            real_path: real_memory_dir.join(&rel_path),
            existed_before: pre_fingerprint.is_some(),
            rel_path,
        });
    }
    changes.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    if changes.is_empty() {
        return Ok(StagedMemoryCommit {
            stats: DreamStats::default(),
            changes,
            backup_root: None,
        });
    }

    let backup_root = backup_real_memory_for_changes(real_memory_dir, &changes).await?;
    let mut rollback_changes = Vec::new();

    for change in &changes {
        rollback_changes.push(change.clone());
        let write_result = async {
            if fail_rel_path.is_some_and(|fail_path| fail_path == change.rel_path.as_path()) {
                return Err(DreamError::Io(std::io::Error::other(
                    "injected write failure",
                )));
            }

            let bytes = tokio::fs::read(&change.staged_path)
                .await
                .map_err(DreamError::Io)?;
            if let Some(parent) = change.real_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(DreamError::Io)?;
            }

            atomic_write_memory_file(&change.real_path, bytes).await
        }
        .await;

        if let Err(write_err) = write_result {
            let rollback_result =
                rollback_staged_memory_changes(&rollback_changes, &backup_root).await;
            cleanup_commit_backup(&backup_root).await;

            let mut message = format!(
                "dream staging commit failed while writing {}: {}",
                change.rel_path.display(),
                write_err
            );
            if let Err(rollback_err) = rollback_result {
                message.push_str(&format!("; rollback failed: {}", rollback_err));
            }
            return Err(DreamError::ConsolidationFailed(message));
        }
    }

    let created = changes
        .iter()
        .filter(|change| !change.existed_before)
        .count();
    let updated = changes
        .iter()
        .filter(|change| change.existed_before)
        .count();

    Ok(StagedMemoryCommit {
        stats: DreamStats {
            memories_created: created,
            memories_updated: updated,
            memories_deleted: 0,
            ..Default::default()
        },
        changes,
        backup_root: Some(backup_root),
    })
}

pub(crate) async fn atomic_write_memory_file(
    path: &Path,
    bytes: Vec<u8>,
) -> Result<(), DreamError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || blockcell_agent::fs_util::atomic_write(&path, &bytes))
        .await
        .map_err(|e| DreamError::Io(std::io::Error::other(e.to_string())))?
        .map_err(DreamError::Io)
}

pub(crate) async fn backup_real_memory_for_changes(
    real_memory_dir: &Path,
    changes: &[StagedMemoryChange],
) -> Result<PathBuf, DreamError> {
    let backup_root = real_memory_dir
        .parent()
        .unwrap_or(real_memory_dir)
        .join(DREAM_COMMIT_BACKUP_DIR)
        .join(format!("run_{}", uuid::Uuid::new_v4().simple()));

    tokio::fs::create_dir_all(&backup_root)
        .await
        .map_err(DreamError::Io)?;

    for change in changes {
        if !change.existed_before {
            continue;
        }

        let backup_path = backup_root.join(&change.rel_path);
        if let Some(parent) = backup_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(DreamError::Io)?;
        }
        tokio::fs::copy(&change.real_path, &backup_path)
            .await
            .map_err(DreamError::Io)?;
    }

    write_commit_manifest(&backup_root, changes).await?;

    Ok(backup_root)
}

pub(crate) async fn write_commit_manifest(
    backup_root: &Path,
    changes: &[StagedMemoryChange],
) -> Result<(), DreamError> {
    let manifest = DreamCommitManifest {
        changes: changes
            .iter()
            .map(|change| DreamCommitManifestChange {
                rel_path: change.rel_path.clone(),
                existed_before: change.existed_before,
            })
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| DreamError::Io(std::io::Error::other(e.to_string())))?;
    atomic_write_memory_file(&backup_root.join(DREAM_COMMIT_MANIFEST_FILE), bytes).await
}

pub(crate) async fn read_commit_manifest(
    backup_root: &Path,
) -> Result<DreamCommitManifest, DreamError> {
    let bytes = tokio::fs::read(backup_root.join(DREAM_COMMIT_MANIFEST_FILE))
        .await
        .map_err(DreamError::Io)?;
    serde_json::from_slice(&bytes).map_err(|e| {
        DreamError::ConsolidationFailed(format!("invalid dream commit manifest: {}", e))
    })
}

pub(crate) fn changes_from_manifest(
    real_memory_dir: &Path,
    manifest: DreamCommitManifest,
) -> Vec<StagedMemoryChange> {
    manifest
        .changes
        .into_iter()
        .map(|change| StagedMemoryChange {
            staged_path: PathBuf::new(),
            real_path: real_memory_dir.join(&change.rel_path),
            rel_path: change.rel_path,
            existed_before: change.existed_before,
        })
        .collect()
}

pub(crate) async fn rollback_staged_memory_changes(
    changes: &[StagedMemoryChange],
    backup_root: &Path,
) -> Result<(), DreamError> {
    let mut errors = Vec::new();

    for change in changes.iter().rev() {
        let result = if change.existed_before {
            let backup_path = backup_root.join(&change.rel_path);
            match tokio::fs::read(&backup_path).await {
                Ok(bytes) => {
                    if let Some(parent) = change.real_path.parent() {
                        if let Err(err) = tokio::fs::create_dir_all(parent).await {
                            Err(DreamError::Io(err))
                        } else {
                            atomic_write_memory_file(&change.real_path, bytes).await
                        }
                    } else {
                        atomic_write_memory_file(&change.real_path, bytes).await
                    }
                }
                Err(err) => Err(DreamError::Io(err)),
            }
        } else {
            match tokio::fs::remove_file(&change.real_path).await {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(DreamError::Io(err)),
            }
        };

        if let Err(err) = result {
            errors.push(format!("{}: {}", change.rel_path.display(), err));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(DreamError::ConsolidationFailed(errors.join("; ")))
    }
}

pub(crate) async fn cleanup_commit_backup(backup_root: &Path) {
    let _ = tokio::fs::remove_dir_all(backup_root).await;
    if let Some(parent) = backup_root.parent() {
        let _ = tokio::fs::remove_dir(parent).await;
    }
}

pub(crate) async fn rollback_pending_memory_commit(
    pending_commit: &mut Option<StagedMemoryCommit>,
) {
    if let Some(commit) = pending_commit.take() {
        if let Err(e) = commit.rollback().await {
            tracing::error!(
                error = %e,
                "[dream] Failed to roll back staged memory commit after final state save failure"
            );
        }
    }
}

pub(crate) async fn recover_dream_commit_backups(
    config_dir: &Path,
    real_memory_dir: &Path,
    roll_back_pending: bool,
) -> Result<(), DreamError> {
    let backup_parent = real_memory_dir
        .parent()
        .unwrap_or(real_memory_dir)
        .join(DREAM_COMMIT_BACKUP_DIR);

    if !fs::try_exists(&backup_parent)
        .await
        .map_err(DreamError::Io)?
    {
        return Ok(());
    }

    let mut entries = fs::read_dir(&backup_parent).await.map_err(DreamError::Io)?;
    while let Some(entry) = entries.next_entry().await.map_err(DreamError::Io)? {
        if !entry.file_type().await.map_err(DreamError::Io)?.is_dir() {
            continue;
        }

        let backup_root = entry.path();
        if roll_back_pending {
            let manifest = match read_commit_manifest(&backup_root).await {
                Ok(manifest) => manifest,
                Err(e) => {
                    tracing::warn!(
                        path = %backup_root.display(),
                        error = %e,
                        "[dream] Found dream commit backup without readable manifest; leaving it for manual recovery"
                    );
                    continue;
                }
            };
            let changes = changes_from_manifest(real_memory_dir, manifest);
            rollback_staged_memory_changes(&changes, &backup_root).await?;
            tracing::warn!(
                path = %backup_root.display(),
                "[dream] Rolled back stale dream memory commit from manifest"
            );
        }

        cleanup_commit_backup(&backup_root).await;
    }

    let _ = tokio::fs::remove_dir(&backup_parent).await;
    tracing::debug!(
        path = %config_dir.display(),
        rollback = roll_back_pending,
        "[dream] Recovered dream commit backup directory"
    );
    Ok(())
}
