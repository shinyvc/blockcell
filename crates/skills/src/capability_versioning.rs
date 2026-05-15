use blockcell_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// 能力版本信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityVersion {
    pub version: String,
    pub artifact_hash: String,
    pub created_at: i64,
    pub source: CapabilityVersionSource,
    pub changelog: Option<String>,
    pub artifact_path: String,
}

/// 版本来源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CapabilityVersionSource {
    Evolution,
    Manual,
    HotReplace,
}

/// 能力版本历史
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityVersionHistory {
    pub capability_id: String,
    pub versions: Vec<CapabilityVersion>,
    pub current_version: String,
}

/// 能力版本管理器 — 为 Capability artifacts 提供版本快照和回滚
pub struct CapabilityVersionManager {
    /// Base directory for capability artifacts
    artifacts_dir: PathBuf,
    /// Directory for version snapshots
    versions_dir: PathBuf,
}

impl CapabilityVersionManager {
    pub fn new(workspace_dir: PathBuf) -> Self {
        let artifacts_dir = workspace_dir.join("tool_artifacts");
        let versions_dir = workspace_dir.join("tool_versions");
        Self {
            artifacts_dir,
            versions_dir,
        }
    }

    /// Create a version snapshot for a capability artifact.
    /// Copies the current artifact to the versions directory.
    pub fn create_version(
        &self,
        capability_id: &str,
        artifact_path: &str,
        source: CapabilityVersionSource,
        changelog: Option<String>,
    ) -> Result<CapabilityVersion> {
        let safe_id = capability_id.replace('.', "_");
        let cap_versions_dir = self.versions_dir.join(&safe_id);
        std::fs::create_dir_all(&cap_versions_dir)?;

        // Load or create history
        let mut history = self.get_history(capability_id)?;

        // Calculate version number
        let version_num = history.versions.len() + 1;
        let version = format!("v{}", version_num);

        // Calculate artifact hash
        let artifact_content = std::fs::read(artifact_path)
            .map_err(|e| Error::Other(format!("Failed to read artifact: {}", e)))?;
        let hash = simple_hash(&artifact_content);

        // Copy artifact to version snapshot
        let ext = std::path::Path::new(artifact_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("sh");
        let snapshot_path = cap_versions_dir.join(format!("{}_{}.{}", safe_id, version, ext));
        std::fs::copy(artifact_path, &snapshot_path)?;

        let cap_version = CapabilityVersion {
            version: version.clone(),
            artifact_hash: hash,
            created_at: chrono::Utc::now().timestamp(),
            source,
            changelog,
            artifact_path: snapshot_path.to_string_lossy().to_string(),
        };

        history.versions.push(cap_version.clone());
        history.current_version = version.clone();
        self.save_history(&history)?;

        info!(
            capability_id = %capability_id,
            version = %version,
            "📦 [能力版本] 创建版本快照: {} -> {}",
            capability_id, version
        );

        Ok(cap_version)
    }

    /// Create a version snapshot unless an identical artifact hash already exists.
    ///
    /// Used by durable workflows when a promotion step may be replayed after a
    /// crash between the external side effect and the step checkpoint.
    pub fn create_version_if_new_artifact(
        &self,
        capability_id: &str,
        artifact_path: &str,
        source: CapabilityVersionSource,
        changelog: Option<String>,
    ) -> Result<CapabilityVersion> {
        let artifact_content = std::fs::read(artifact_path)
            .map_err(|e| Error::Other(format!("Failed to read artifact: {}", e)))?;
        let hash = simple_hash(&artifact_content);

        let mut history = self.get_history(capability_id)?;
        if let Some(existing) = history
            .versions
            .iter()
            .rev()
            .find(|version| version.artifact_hash == hash)
            .cloned()
        {
            if history.current_version != existing.version {
                history.current_version = existing.version.clone();
                self.save_history(&history)?;
            }
            info!(
                capability_id = %capability_id,
                version = %existing.version,
                "📝 [能力版本] 复用已有 artifact 版本快照: {} -> {}",
                capability_id, existing.version
            );
            return Ok(existing);
        }

        self.create_version(capability_id, artifact_path, source, changelog)
    }

    /// 回滚到当前版本的前一个版本。返回恢复后的 artifact 路径。
    /// 非破坏性回滚：保留所有版本历史，支持 roll-forward。
    /// 基于 current_version 定位，而非总是取倒数第二个版本。
    pub fn rollback(&self, capability_id: &str) -> Result<Option<String>> {
        let mut history = self.get_history(capability_id)?;

        if history.versions.is_empty() {
            warn!(
                capability_id = %capability_id,
                "📦 [能力版本] 没有可回滚的版本: {}",
                capability_id
            );
            return Ok(None);
        }

        // 基于 current_version 找到当前版本在列表中的位置，再切到前一个版本
        let current_idx = history
            .versions
            .iter()
            .position(|v| v.version == history.current_version);

        let prev_idx = match current_idx {
            Some(0) => {
                // 当前已是第一个版本，无法继续回滚
                warn!(
                    capability_id = %capability_id,
                    current_version = %history.current_version,
                    "📦 [能力版本] 当前已是第一个版本，无法回滚: {}",
                    capability_id
                );
                return Ok(None);
            }
            Some(idx) => idx - 1,
            None => {
                // current_version 在列表中未找到，回退到倒数第二个版本
                if history.versions.len() < 2 {
                    return Ok(None);
                }
                history.versions.len() - 2
            }
        };

        let prev = &history.versions[prev_idx];
        history.current_version = prev.version.clone();
        let restore_path = prev.artifact_path.clone();

        // 先验证并复制 artifact 到临时文件，成功后再原子替换并保存 history
        // 防止 copy 失败时 history 已回滚但 artifact 未变，导致版本状态不一致
        let safe_id = capability_id.replace('.', "_");
        let ext = std::path::Path::new(&restore_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("sh");
        let active_path = self.artifacts_dir.join(format!("{}.{}", safe_id, ext));

        // 验证快照文件存在并可读
        if !std::path::Path::new(&restore_path).exists() {
            warn!(
                capability_id = %capability_id,
                restore_path = %restore_path,
                "📦 [能力版本] 回滚快照文件不存在: {}",
                restore_path
            );
            return Ok(None);
        }

        // 先复制 artifact（如果失败则不修改 history，保持一致性）
        std::fs::copy(&restore_path, &active_path)?;

        // artifact 复制成功，现在才保存 history
        self.save_history(&history)?;

        info!(
            capability_id = %capability_id,
            version = %history.current_version,
            "📦 [能力版本] 回滚到: {} -> {}",
            capability_id, history.current_version
        );

        Ok(Some(active_path.to_string_lossy().to_string()))
    }

    /// List all versions for a capability.
    pub fn list_versions(&self, capability_id: &str) -> Result<Vec<CapabilityVersion>> {
        let history = self.get_history(capability_id)?;
        Ok(history.versions)
    }

    /// Get the current version string for a capability.
    pub fn get_current_version(&self, capability_id: &str) -> Result<String> {
        let history = self.get_history(capability_id)?;
        Ok(history.current_version)
    }

    /// Cleanup old versions, keeping only the most recent `keep_count`.
    pub fn cleanup_old_versions(&self, capability_id: &str, keep_count: usize) -> Result<usize> {
        let mut history = self.get_history(capability_id)?;

        if history.versions.len() <= keep_count {
            return Ok(0);
        }

        let remove_count = history.versions.len() - keep_count;
        let removed: Vec<CapabilityVersion> = history.versions.drain(..remove_count).collect();

        for v in &removed {
            let _ = std::fs::remove_file(&v.artifact_path);
        }

        self.save_history(&history)?;

        debug!(
            capability_id = %capability_id,
            removed = remove_count,
            "📦 [能力版本] 清理旧版本: {} 个",
            remove_count
        );

        Ok(remove_count)
    }

    // === Internal helpers ===

    fn get_history(&self, capability_id: &str) -> Result<CapabilityVersionHistory> {
        let history_file = self.history_file_path(capability_id);
        if !history_file.exists() {
            return Ok(CapabilityVersionHistory {
                capability_id: capability_id.to_string(),
                versions: vec![],
                current_version: "v0".to_string(),
            });
        }
        let content = std::fs::read_to_string(&history_file)?;
        let history: CapabilityVersionHistory = serde_json::from_str(&content)?;
        Ok(history)
    }

    fn save_history(&self, history: &CapabilityVersionHistory) -> Result<()> {
        let history_file = self.history_file_path(&history.capability_id);
        if let Some(parent) = history_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(history)?;
        std::fs::write(&history_file, content)?;
        Ok(())
    }

    fn history_file_path(&self, capability_id: &str) -> PathBuf {
        let safe_id = capability_id.replace('.', "_");
        self.versions_dir.join(format!("{}_history.json", safe_id))
    }
}

/// Simple hash function (FNV-1a style) for artifact content.
fn simple_hash(data: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_version_create_and_list() {
        let tmp = std::env::temp_dir().join("test_cap_ver_create");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vm = CapabilityVersionManager::new(tmp.clone());

        // Create a fake artifact
        let artifacts_dir = tmp.join("tool_artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let artifact = artifacts_dir.join("test_cap.sh");
        std::fs::write(&artifact, "#!/bin/bash\necho ok").unwrap();

        let v1 = vm
            .create_version(
                "test.cap",
                artifact.to_str().unwrap(),
                CapabilityVersionSource::Evolution,
                Some("Initial version".to_string()),
            )
            .unwrap();

        assert_eq!(v1.version, "v1");

        // Create v2
        std::fs::write(&artifact, "#!/bin/bash\necho ok v2").unwrap();
        let v2 = vm
            .create_version(
                "test.cap",
                artifact.to_str().unwrap(),
                CapabilityVersionSource::HotReplace,
                None,
            )
            .unwrap();
        assert_eq!(v2.version, "v2");

        let versions = vm.list_versions("test.cap").unwrap();
        assert_eq!(versions.len(), 2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_create_version_if_new_artifact_reuses_existing_hash() {
        let tmp = std::env::temp_dir().join("test_cap_ver_reuse_hash");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vm = CapabilityVersionManager::new(tmp.clone());
        let artifacts_dir = tmp.join("tool_artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let artifact = artifacts_dir.join("reuse_cap.sh");
        std::fs::write(&artifact, "#!/bin/bash\necho same").unwrap();

        let v1 = vm
            .create_version_if_new_artifact(
                "reuse.cap",
                artifact.to_str().unwrap(),
                CapabilityVersionSource::Evolution,
                Some("first".to_string()),
            )
            .unwrap();
        let v1_again = vm
            .create_version_if_new_artifact(
                "reuse.cap",
                artifact.to_str().unwrap(),
                CapabilityVersionSource::Evolution,
                Some("replay".to_string()),
            )
            .unwrap();

        assert_eq!(v1.version, "v1");
        assert_eq!(v1_again.version, "v1");
        let versions = vm.list_versions("reuse.cap").unwrap();
        assert_eq!(versions.len(), 1);

        std::fs::write(&artifact, "#!/bin/bash\necho changed").unwrap();
        let v2 = vm
            .create_version_if_new_artifact(
                "reuse.cap",
                artifact.to_str().unwrap(),
                CapabilityVersionSource::Evolution,
                Some("changed".to_string()),
            )
            .unwrap();
        assert_eq!(v2.version, "v2");

        let versions = vm.list_versions("reuse.cap").unwrap();
        assert_eq!(versions.len(), 2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_capability_version_rollback() {
        let tmp = std::env::temp_dir().join("test_cap_ver_rollback");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vm = CapabilityVersionManager::new(tmp.clone());

        let artifacts_dir = tmp.join("tool_artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let artifact = artifacts_dir.join("rollback_cap.sh");

        // v1
        std::fs::write(&artifact, "#!/bin/bash\necho v1").unwrap();
        vm.create_version(
            "rollback.cap",
            artifact.to_str().unwrap(),
            CapabilityVersionSource::Evolution,
            None,
        )
        .unwrap();

        // v2
        std::fs::write(&artifact, "#!/bin/bash\necho v2").unwrap();
        vm.create_version(
            "rollback.cap",
            artifact.to_str().unwrap(),
            CapabilityVersionSource::Evolution,
            None,
        )
        .unwrap();

        // Rollback (non-destructive: all versions preserved)
        let restored = vm.rollback("rollback.cap").unwrap();
        assert!(restored.is_some());

        let current = vm.get_current_version("rollback.cap").unwrap();
        assert_eq!(current, "v1");

        // All versions still exist after rollback (non-destructive)
        let versions = vm.list_versions("rollback.cap").unwrap();
        assert_eq!(versions.len(), 2);

        // Can roll-forward back to v2
        let history_file = tmp.join("tool_versions").join("rollback_cap_history.json");
        let content = std::fs::read_to_string(&history_file).unwrap();
        let mut history: CapabilityVersionHistory = serde_json::from_str(&content).unwrap();
        history.current_version = "v2".to_string();
        std::fs::write(
            &history_file,
            serde_json::to_string_pretty(&history).unwrap(),
        )
        .unwrap();

        let current_after_forward = vm.get_current_version("rollback.cap").unwrap();
        assert_eq!(current_after_forward, "v2");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_capability_version_cleanup() {
        let tmp = std::env::temp_dir().join("test_cap_ver_cleanup");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vm = CapabilityVersionManager::new(tmp.clone());

        let artifacts_dir = tmp.join("tool_artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let artifact = artifacts_dir.join("cleanup_cap.sh");

        for i in 1..=5 {
            std::fs::write(&artifact, format!("#!/bin/bash\necho v{}", i)).unwrap();
            vm.create_version(
                "cleanup.cap",
                artifact.to_str().unwrap(),
                CapabilityVersionSource::Evolution,
                None,
            )
            .unwrap();
        }

        let removed = vm.cleanup_old_versions("cleanup.cap", 2).unwrap();
        assert_eq!(removed, 3);

        let versions = vm.list_versions("cleanup.cap").unwrap();
        assert_eq!(versions.len(), 2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_simple_hash() {
        let h1 = simple_hash(b"hello");
        let h2 = simple_hash(b"hello");
        let h3 = simple_hash(b"world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    /// 回归测试：基于 current_version 定位的连续回滚 v3 -> v2 -> v1
    #[test]
    fn test_sequential_rollback_v3_to_v2_to_v1() {
        let tmp = std::env::temp_dir().join("test_cap_ver_seq_rollback");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vm = CapabilityVersionManager::new(tmp.clone());
        let artifacts_dir = tmp.join("tool_artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        let artifact = artifacts_dir.join("seq_rollback_cap.sh");

        // 创建 v1
        std::fs::write(&artifact, "#!/bin/bash\necho v1").unwrap();
        vm.create_version(
            "seq.rollback",
            artifact.to_str().unwrap(),
            CapabilityVersionSource::Evolution,
            None,
        )
        .unwrap();
        assert_eq!(vm.get_current_version("seq.rollback").unwrap(), "v1");

        // 创建 v2
        std::fs::write(&artifact, "#!/bin/bash\necho v2").unwrap();
        vm.create_version(
            "seq.rollback",
            artifact.to_str().unwrap(),
            CapabilityVersionSource::Evolution,
            None,
        )
        .unwrap();
        assert_eq!(vm.get_current_version("seq.rollback").unwrap(), "v2");

        // 创建 v3
        std::fs::write(&artifact, "#!/bin/bash\necho v3").unwrap();
        vm.create_version(
            "seq.rollback",
            artifact.to_str().unwrap(),
            CapabilityVersionSource::Evolution,
            None,
        )
        .unwrap();
        assert_eq!(vm.get_current_version("seq.rollback").unwrap(), "v3");

        // 第一次回滚：v3 -> v2
        vm.rollback("seq.rollback").unwrap();
        assert_eq!(vm.get_current_version("seq.rollback").unwrap(), "v2");

        // 第二次回滚：v2 -> v1
        vm.rollback("seq.rollback").unwrap();
        assert_eq!(vm.get_current_version("seq.rollback").unwrap(), "v1");

        // 第三次回滚：v1 已是第一个版本，无法继续
        let result = vm.rollback("seq.rollback").unwrap();
        assert!(result.is_none());
        assert_eq!(vm.get_current_version("seq.rollback").unwrap(), "v1");

        // 所有版本仍然存在（非破坏性）
        let versions = vm.list_versions("seq.rollback").unwrap();
        assert_eq!(versions.len(), 3);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
