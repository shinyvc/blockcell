use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use blockcell_core::Paths;
use blockcell_tools::mcp::manager::{mcp_access_allows, McpManager, McpToolTarget};
use serde_json::json;
use tokio::time::timeout;
use uuid::Uuid;

#[test]
fn mcp_manager_parses_qualified_tool_name() {
    let parsed = McpToolTarget::parse("github__list_issues").expect("qualified mcp tool");

    assert_eq!(parsed.server_name, "github");
    assert_eq!(parsed.tool_name, "list_issues");
}

#[test]
fn mcp_manager_rejects_unqualified_tool_name() {
    assert!(McpToolTarget::parse("list_issues").is_none());
    assert!(McpToolTarget::parse("github__").is_none());
}

#[test]
fn mcp_access_allows_exact_tool_even_without_server_allowlist() {
    let allowed = mcp_access_allows(
        &[],
        &["github__list_issues".to_string()],
        "github__list_issues",
    );

    assert!(allowed);
}

#[test]
fn mcp_access_allows_server_wildcard_visibility() {
    let allowed = mcp_access_allows(&["github".to_string()], &[], "github__search_repositories");

    assert!(allowed);
}

#[test]
fn mcp_access_denies_when_no_rules_match() {
    let allowed = mcp_access_allows(&["sqlite".to_string()], &[], "github__list_issues");

    assert!(!allowed);
}

struct TestMcpHome {
    base: PathBuf,
}

impl TestMcpHome {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("blockcell-mcp-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).expect("create temp mcp home");
        Self { base }
    }

    fn paths(&self) -> Paths {
        Paths::with_base(self.base.clone())
    }
}

impl Drop for TestMcpHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn slow_mcp_script(init_delay_secs: u64, call_delay_secs: u64) -> String {
    format!(
        r#"import json, sys, time

def respond(req_id, result):
    sys.stdout.write(json.dumps({{"jsonrpc": "2.0", "id": req_id, "result": result}}) + "\n")
    sys.stdout.flush()

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    method = msg.get("method")
    if method == "initialize":
        time.sleep({init_delay_secs})
        respond(msg["id"], {{
            "protocolVersion": "2024-11-05",
            "capabilities": {{}},
            "serverInfo": {{"name": "slow", "version": "1.0"}}
        }})
    elif method == "tools/list":
        respond(msg["id"], {{
            "tools": [{{
                "name": "slow_echo",
                "description": "Slow echo",
                "inputSchema": {{"type": "object"}}
            }}]
        }})
    elif method == "tools/call":
        time.sleep({call_delay_secs})
        respond(msg["id"], {{
            "content": [{{"type": "text", "text": "ok"}}],
            "isError": False
        }})
"#
    )
}

fn write_test_mcp_config(
    paths: &Paths,
    init_delay_secs: u64,
    call_delay_secs: u64,
    startup_timeout_secs: u64,
    call_timeout_secs: u64,
) {
    let args = vec![
        "-u".to_string(),
        "-c".to_string(),
        slow_mcp_script(init_delay_secs, call_delay_secs),
    ];

    let config = json!({
        "servers": {
            "slow": {
                "command": "python3",
                "args": args,
                "enabled": true,
                "autoStart": true,
                "startupTimeoutSecs": startup_timeout_secs,
                "callTimeoutSecs": call_timeout_secs
            }
        }
    });

    fs::write(
        paths.mcp_config_file(),
        serde_json::to_string_pretty(&config).expect("serialize mcp config"),
    )
    .expect("write mcp config");
}

#[tokio::test]
async fn mcp_manager_bounds_auto_start_with_startup_timeout() {
    let home = TestMcpHome::new();
    let paths = home.paths();
    paths.ensure_dirs().expect("ensure dirs");
    write_test_mcp_config(&paths, 2, 0, 1, 1);

    let started = Instant::now();
    let manager = timeout(Duration::from_secs(4), McpManager::load(&paths))
        .await
        .expect("manager load should not hang")
        .expect("manager load should succeed even if auto-start fails");
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_secs(2));

    let started = Instant::now();
    let result = timeout(Duration::from_secs(4), manager.client_for("slow"))
        .await
        .expect("client_for should not hang");
    let elapsed = started.elapsed();

    assert!(result.is_err(), "startup should time out");
    assert!(elapsed < Duration::from_secs(2));
    let err = result.err().expect("startup timeout error").to_string();
    assert!(
        err.contains("timeout") || err.contains("timed out"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn mcp_client_respects_call_timeout() {
    let home = TestMcpHome::new();
    let paths = home.paths();
    paths.ensure_dirs().expect("ensure dirs");
    write_test_mcp_config(&paths, 0, 2, 1, 1);

    let manager = timeout(Duration::from_secs(4), McpManager::load(&paths))
        .await
        .expect("manager load should not hang")
        .expect("manager should start when handshake is fast");
    let client = timeout(Duration::from_secs(4), manager.client_for("slow"))
        .await
        .expect("client_for should not hang")
        .expect("mcp client");

    let started = Instant::now();
    let result = timeout(
        Duration::from_secs(4),
        client.call_tool("slow_echo", json!({"message": "hi"})),
    )
    .await;
    let elapsed = started.elapsed();

    assert!(result.is_ok(), "tool call hung for {elapsed:?}");
    let call_result = result.expect("tool call finished before harness timeout");
    assert!(call_result.is_err(), "tool call should time out");
    assert!(elapsed < Duration::from_secs(2));
    let err = call_result.err().expect("call timeout error").to_string();
    assert!(
        err.contains("timeout") || err.contains("timed out"),
        "unexpected error: {err}"
    );
}
