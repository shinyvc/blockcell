//! 技能目录布局探测辅助函数。
//!
//! 在技能目录中查找遗留 Python 脚本 / 本地脚本资产，并判断文件是否可执行，
//! 供 `detect_skill_layout` 判定技能布局类型时使用。

use std::path::{Path, PathBuf};

use super::EvolutionService;

impl EvolutionService {
    pub(super) fn first_legacy_python_script(skill_dir: &Path) -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        let scripts_dir = skill_dir.join("scripts");
        if scripts_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&scripts_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().is_some_and(|e| e == "py") {
                        candidates.push(path);
                    }
                }
            }
        }

        if candidates.is_empty() {
            if let Ok(entries) = std::fs::read_dir(skill_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file()
                        && path.file_name().and_then(|n| n.to_str()) != Some("SKILL.py")
                        && path.extension().is_some_and(|e| e == "py")
                    {
                        candidates.push(path);
                    }
                }
            }
        }

        candidates.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
        candidates.into_iter().next()
    }

    pub(super) fn first_local_script_asset(skill_dir: &Path) -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();

        let allowed_extensions = ["sh", "bash", "zsh", "js", "php", "rb"];
        let scan_dir = |dir: &Path, candidates: &mut Vec<PathBuf>| {
            if !dir.is_dir() {
                return;
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let ext_ok = path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| allowed_extensions.contains(&ext));
                    let no_ext_exec = path.extension().is_none() && Self::looks_executable(&path);
                    if ext_ok || no_ext_exec {
                        candidates.push(path);
                    }
                }
            }
        };

        scan_dir(&skill_dir.join("scripts"), &mut candidates);
        scan_dir(&skill_dir.join("bin"), &mut candidates);

        if candidates.is_empty() {
            if let Ok(entries) = std::fs::read_dir(skill_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if matches!(
                        file_name,
                        "SKILL.md" | "SKILL.py" | "SKILL.rhai" | "meta.yaml" | "meta.json"
                    ) {
                        continue;
                    }
                    let ext_ok = path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| allowed_extensions.contains(&ext));
                    let no_ext_exec = path.extension().is_none() && Self::looks_executable(&path);
                    if ext_ok || no_ext_exec {
                        candidates.push(path);
                    }
                }
            }
        }

        candidates.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
        candidates.into_iter().next()
    }

    #[cfg(unix)]
    pub(super) fn looks_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    pub(super) fn looks_executable(_path: &Path) -> bool {
        false
    }
}
