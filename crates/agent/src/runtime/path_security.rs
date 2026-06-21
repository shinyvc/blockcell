//! 路径安全检查 — AgentRuntime 的文件系统访问控制方法
//!
//! 包含路径提取、安全校验、用户授权和危险操作确认。

use super::{canonical_or_normalized, is_path_within_base, ConfirmRequest};
use blockcell_core::path_policy::{PathOp, PolicyAction};
use blockcell_core::InboundMessage;
use std::path::{Component, Path, PathBuf};
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
                // Also subject filesystem paths referenced inside the command
                // itself to the path policy, so built-in sensitive paths
                // (~/.ssh, /etc, ...) are enforced for `exec`, not just for
                // its working directory.
                if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                    paths.extend(extract_command_paths(cmd));
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
            // These run scripts addressed relative to the active skill
            // directory, so the generic workspace policy doesn't apply. Still
            // enforce — at the runtime layer — that the script path is a safe
            // relative path that cannot escape the skill scope.
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if !is_safe_relative_skill_path(path) {
                warn!(
                    tool = tool_name,
                    path, "Rejecting exec script path that escapes skill scope"
                );
                return false;
            }
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
                agent_id: self.agent_id.clone(),
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                ws_connection_id: msg
                    .metadata
                    .get("ws_connection_id")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string()),
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
                agent_id: self.agent_id.clone(),
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                ws_connection_id: msg
                    .metadata
                    .get("ws_connection_id")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string()),
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

/// Heuristically extract filesystem-path-looking tokens from a shell command.
///
/// Used to subject paths referenced inside an `exec` command to the path
/// policy. This is intentionally conservative: when in doubt a token is
/// treated as a path, so the policy (deny / confirm) gets a chance to run.
/// Flags (`-x`), URLs (`scheme://...`), and `key=value` tokens are skipped.
fn extract_command_paths(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in command.split_whitespace() {
        let tok = raw.trim_matches(|c| c == '"' || c == '\'');
        if tok.is_empty() || tok.starts_with('-') {
            continue;
        }
        if tok.contains("://") {
            continue;
        }
        let looks_like_path = tok.starts_with('/')
            || tok.starts_with("~/")
            || tok == "~"
            || tok.starts_with("./")
            || tok.starts_with("../")
            || (tok.contains('/') && !tok.contains('='));
        if looks_like_path {
            out.push(tok.to_string());
        }
    }
    out
}

/// Whether `path` is a non-empty relative path that stays inside the active
/// skill directory (no absolute paths, no `..` traversal). Mirrors the
/// `exec_local`/`exec_skill_script` tools' own validation as defense in depth.
fn is_safe_relative_skill_path(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return false;
    }
    !candidate.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{extract_command_paths, is_safe_relative_skill_path};

    #[test]
    fn accepts_safe_relative_skill_paths() {
        for p in ["scripts/hello.sh", "main.py", "./run.sh", "a/b/c.py"] {
            assert!(is_safe_relative_skill_path(p), "expected `{p}` accepted");
        }
    }

    #[test]
    fn rejects_absolute_or_escaping_skill_paths() {
        for p in [
            "",
            "   ",
            "/etc/passwd",
            "../secret.sh",
            "scripts/../../etc/passwd",
        ] {
            assert!(!is_safe_relative_skill_path(p), "expected `{p}` rejected");
        }
    }

    #[test]
    fn extracts_absolute_and_home_paths() {
        assert_eq!(
            extract_command_paths("cat /etc/shadow"),
            vec!["/etc/shadow".to_string()]
        );
        assert_eq!(
            extract_command_paths("cat ~/.ssh/id_rsa"),
            vec!["~/.ssh/id_rsa".to_string()]
        );
        assert_eq!(
            extract_command_paths("cp 'a' \"/etc/hosts\""),
            vec!["/etc/hosts".to_string()]
        );
    }

    #[test]
    fn extracts_relative_paths_with_slash() {
        assert_eq!(
            extract_command_paths("python src/main.py"),
            vec!["src/main.py".to_string()]
        );
        assert_eq!(
            extract_command_paths("ls ./build ../out"),
            vec!["./build".to_string(), "../out".to_string()]
        );
    }

    #[test]
    fn skips_flags_urls_and_kv_and_bare_words() {
        assert!(extract_command_paths("ls -la").is_empty());
        assert!(extract_command_paths("echo hello world").is_empty());
        assert!(extract_command_paths("curl https://example.com/x").is_empty());
        assert!(extract_command_paths("git log --format=%H").is_empty());
    }
}
