//! 路径安全检查 — AgentRuntime 的文件系统访问控制方法
//!
//! 包含路径提取、安全校验、用户授权和危险操作确认。

use super::{canonical_or_normalized, is_path_within_base, ConfirmRequest};
use blockcell_core::path_policy::{PathOp, PolicyAction};
use blockcell_core::InboundMessage;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

impl super::AgentRuntime {
    /// Extract filesystem paths from tool call parameters.
    pub(super) fn extract_paths(&self, tool_name: &str, args: &serde_json::Value) -> Vec<String> {
        let mut paths = Vec::new();
        match tool_name {
            "read_file" | "write_file" | "edit_file" | "list_dir" => {
                if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
                    paths.push(p.to_string());
                }
            }
            "file_ops" | "data_process" | "audio_transcribe" | "chart_generate"
            | "office_write" | "video_process" | "health_api" | "encrypt" => {
                if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
                    paths.push(p.to_string());
                }
                if let Some(d) = args.get("destination").and_then(|v| v.as_str()) {
                    paths.push(d.to_string());
                }
                if let Some(o) = args.get("output_path").and_then(|v| v.as_str()) {
                    paths.push(o.to_string());
                }
                if let Some(arr) = args.get("paths").and_then(|v| v.as_array()) {
                    for p in arr {
                        if let Some(s) = p.as_str() {
                            paths.push(s.to_string());
                        }
                    }
                }
            }
            "message" => {
                if let Some(arr) = args.get("media").and_then(|v| v.as_array()) {
                    for p in arr {
                        if let Some(s) = p.as_str() {
                            paths.push(s.to_string());
                        }
                    }
                }
            }
            "browse" => {
                if let Some(o) = args.get("output_path").and_then(|v| v.as_str()) {
                    paths.push(o.to_string());
                }
            }
            "exec" => {
                if let Some(wd) = args.get("working_dir").and_then(|v| v.as_str()) {
                    paths.push(wd.to_string());
                }
            }
            _ => {}
        }
        paths
    }

    /// Resolve a path string the same way tools do (expand ~ and relative paths).
    pub(super) fn resolve_path(&self, path_str: &str) -> PathBuf {
        if path_str.starts_with("~/") {
            dirs::home_dir()
                .map(|h| h.join(&path_str[2..]))
                .unwrap_or_else(|| PathBuf::from(path_str))
        } else if path_str.starts_with('/') {
            PathBuf::from(path_str)
        } else {
            self.paths.workspace().join(path_str)
        }
    }

    /// Check if a resolved path is inside the safe workspace directory.
    pub(super) fn is_path_safe(&self, resolved: &std::path::Path) -> bool {
        is_path_within_base(&self.paths.workspace(), resolved)
    }

    /// Check whether a resolved path falls within an already-authorized directory.
    /// Optimized (#12): walk ancestors with O(1) HashSet lookups instead of O(n) iteration.
    /// `authorized_dirs` stores already-canonicalized paths, so no re-canonicalization needed.
    pub(super) fn is_path_authorized(&self, resolved: &std::path::Path) -> bool {
        if self.authorized_dirs.is_empty() {
            return false;
        }
        let rp = canonical_or_normalized(resolved);
        let mut current = rp.as_path();
        loop {
            if self.authorized_dirs.contains(current) {
                return true;
            }
            match current.parent() {
                Some(parent) if parent != current => current = parent,
                _ => return false,
            }
        }
    }

    /// Record a directory as authorized so future accesses within it are auto-approved.
    pub(super) fn authorize_directory(&mut self, resolved: &std::path::Path) {
        // If the path is a directory, authorize it directly.
        // If it's a file, authorize its parent directory.
        let dir = if resolved.is_dir() {
            resolved.to_path_buf()
        } else {
            resolved
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| resolved.to_path_buf())
        };
        let dir = canonical_or_normalized(&dir);
        if self.authorized_dirs.insert(dir.clone()) {
            info!(dir = %dir.display(), "Directory authorized for future access");
        }
    }

    /// For tools that access the filesystem, check if any paths are outside the
    /// workspace. Applies the path-access policy first; only paths whose policy
    /// outcome is `Confirm` are forwarded to the user for interactive approval.
    ///
    /// Priority (highest → lowest):
    /// 1. Workspace-safe paths  → always allowed
    /// 2. Session-authorized dirs → allowed (cached from prior confirmation)
    /// 3. Policy `Deny`         → rejected immediately, no confirmation sent
    /// 4. Policy `Allow`        → allowed immediately, cached for this session
    /// 5. Policy `Confirm`      → user confirmation required
    pub(super) async fn check_path_permission(
        &mut self,
        tool_name: &str,
        args: &serde_json::Value,
        msg: &InboundMessage,
    ) -> bool {
        if matches!(tool_name, "exec_local" | "exec_skill_script") {
            return true;
        }
        let raw_paths = self.extract_paths(tool_name, args);
        if raw_paths.is_empty() {
            return true;
        }

        let op = PathOp::from_tool_name(tool_name);

        // Classify each path by policy outcome
        let mut deny_paths: Vec<String> = Vec::new();
        let mut confirm_paths: Vec<String> = Vec::new();

        for p in &raw_paths {
            let resolved = self.resolve_path(p);

            // 1. Workspace-safe → always OK
            if self.is_path_safe(&resolved) {
                continue;
            }

            // 2. Already authorized by user this session → OK
            if self.is_path_authorized(&resolved) {
                continue;
            }

            // 3. Evaluate policy
            let action = self.path_policy.evaluate(&resolved, op);
            match action {
                PolicyAction::Deny => {
                    warn!(
                        tool = tool_name,
                        path = %resolved.display(),
                        "Path access denied by policy"
                    );
                    deny_paths.push(p.clone());
                }
                PolicyAction::Allow => {
                    // Policy explicitly allows — cache for this session
                    info!(
                        tool = tool_name,
                        path = %resolved.display(),
                        "Path access allowed by policy"
                    );
                    if self.path_policy.cache_confirmed_dirs() {
                        self.authorize_directory(&resolved);
                    }
                }
                PolicyAction::Confirm => {
                    confirm_paths.push(p.clone());
                }
            }
        }

        // Any hard-deny → reject the whole operation
        if !deny_paths.is_empty() {
            return false;
        }

        // All paths were allowed (workspace / session-cache / policy-allow)
        if confirm_paths.is_empty() {
            return true;
        }

        // Need user confirmation for the remaining paths
        if let Some(confirm_tx) = &self.confirm_tx {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            let request = ConfirmRequest {
                tool_name: tool_name.to_string(),
                paths: confirm_paths.clone(),
                response_tx,
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
            };

            if confirm_tx.send(request).await.is_err() {
                warn!("Failed to send confirmation request, denying access");
                return false;
            }

            match response_rx.await {
                Ok(allowed) => {
                    if allowed && self.path_policy.cache_confirmed_dirs() {
                        for p in &confirm_paths {
                            let resolved = self.resolve_path(p);
                            self.authorize_directory(&resolved);
                        }
                    }
                    allowed
                }
                Err(_) => {
                    warn!("Confirmation channel closed, denying access");
                    false
                }
            }
        } else {
            warn!(
                tool = tool_name,
                "No confirmation channel, denying access to paths outside workspace"
            );
            false
        }
    }

    pub(super) async fn confirm_dangerous_operation(
        &mut self,
        tool_name: &str,
        items: Vec<String>,
        msg: &InboundMessage,
    ) -> bool {
        if items.is_empty() {
            return true;
        }
        if let Some(confirm_tx) = &self.confirm_tx {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            let request = ConfirmRequest {
                tool_name: tool_name.to_string(),
                paths: items,
                response_tx,
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
            };
            if confirm_tx.send(request).await.is_err() {
                warn!(
                    tool = tool_name,
                    "Failed to send dangerous-operation confirmation request, denying"
                );
                return false;
            }
            match response_rx.await {
                Ok(allowed) => allowed,
                Err(_) => {
                    warn!(
                        tool = tool_name,
                        "Dangerous-operation confirmation channel closed, denying"
                    );
                    false
                }
            }
        } else {
            warn!(
                tool = tool_name,
                "No confirmation channel, denying dangerous operation"
            );
            false
        }
    }
}
