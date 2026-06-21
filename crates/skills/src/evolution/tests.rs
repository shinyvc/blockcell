use super::*;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_skills_dir(tag: &str) -> PathBuf {
    let mut root = std::env::temp_dir();
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    root.push(format!(
        "blockcell_hybrid_prompt_{}_{}_{}",
        tag,
        std::process::id(),
        now_ns
    ));
    std::fs::create_dir_all(&root).expect("create temp skills dir");
    root
}

fn sample_hybrid_context() -> EvolutionContext {
    EvolutionContext {
        skill_name: "hybrid_demo".to_string(),
        current_version: "v1".to_string(),
        trigger: TriggerReason::ManualRequest {
            description: "build a hybrid skill".to_string(),
        },
        error_stack: None,
        source_snippet: Some("print('hello')\n".to_string()),
        source_path: Some("SKILL.py".to_string()),
        layout: SkillLayout::Hybrid,
        tool_schemas: vec![],
        timestamp: chrono::Utc::now().timestamp(),
        skill_type: SkillType::Python,
        staged: false,
        staging_skills_dir: None,
    }
}

#[test]
fn test_hybrid_generation_prompt_mentions_manual_and_entrypoint_boundary() {
    let skills_dir = temp_skills_dir("gen");
    let engine = SkillEvolution::new(skills_dir, 5);
    let prompt = engine
        .build_hybrid_generation_prompt(&sample_hybrid_context())
        .expect("build hybrid generation prompt");

    assert!(prompt.contains("## Hybrid Contract"));
    assert!(prompt.contains("SKILL.md` defines the user-facing behavior"));
    assert!(prompt.contains("## Summary {#summary}"));
    assert!(prompt.contains("Current entrypoint: `SKILL.py`"));
    assert!(prompt.contains("exec_local"));
    assert!(prompt.contains("local execution is appropriate"));
}

#[test]
fn test_hybrid_fix_prompt_mentions_manual_and_entrypoint_boundary() {
    let skills_dir = temp_skills_dir("fix");
    let engine = SkillEvolution::new(skills_dir, 5);
    let feedback = FeedbackEntry {
        attempt: 1,
        stage: "compile".to_string(),
        feedback: "entrypoint mismatch".to_string(),
        previous_code: "print('bad')\n".to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    };

    let prompt = engine
        .build_hybrid_fix_prompt(&sample_hybrid_context(), &feedback, &[])
        .expect("build hybrid fix prompt");

    assert!(prompt.contains("## Hybrid Contract"));
    assert!(prompt.contains("Keep the manual and the entrypoint aligned"));
    assert!(prompt.contains("## Summary {#summary}"));
    assert!(prompt.contains("Current entrypoint: `SKILL.py`"));
    assert!(prompt.contains("exec_local"));
    assert!(prompt.contains("entrypoint mismatch"));
}

#[tokio::test]
async fn test_compile_local_script_detects_python_shebang_for_no_extension_script() {
    let skills_dir = temp_skills_dir("no_ext_shebang");
    let engine = SkillEvolution::new(skills_dir.clone(), 5);
    let skill_path = skills_dir.join("run-me");
    std::fs::write(
        &skill_path,
        "#!/usr/bin/env python3\nprint(\"unterminated\"\n",
    )
    .expect("write shebang script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&skill_path)
            .expect("script metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&skill_path, perms).expect("set executable bit");
    }

    let (passed, error) = engine
        .compile_local_script(&skill_path)
        .await
        .expect("compile should run");

    assert!(!passed);
    let error = error.expect("should return syntax error");
    assert!(error.contains("SyntaxError") || error.contains("unterminated"));
}
