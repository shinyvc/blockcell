use std::collections::HashMap;

use blockcell_agent::intent::IntentToolResolver;
use blockcell_core::mcp_config::{McpDefaultsConfig, McpResolvedConfig, McpServerConfig};
use blockcell_core::Config;
use blockcell_tools::ToolRegistry;

fn resolved_mcp(server_name: &str) -> McpResolvedConfig {
    let mut servers = HashMap::new();
    servers.insert(
        server_name.to_string(),
        McpServerConfig {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "dummy".to_string()],
            env: HashMap::new(),
            cwd: None,
            enabled: true,
            auto_start: true,
            startup_timeout_secs: 20,
            call_timeout_secs: 60,
        },
    );
    McpResolvedConfig {
        defaults: McpDefaultsConfig::default(),
        servers,
    }
}

#[test]
fn intent_validation_accepts_declared_mcp_tool_when_server_exists() {
    let raw = r#"{
  "intentRouter": {
    "enabled": true,
    "defaultProfile": "default",
    "profiles": {
      "default": {
        "coreTools": ["read_file"],
        "intentTools": {
          "Chat": { "inheritBase": false, "tools": [] },
          "Unknown": ["github__search_repositories"]
        }
      }
    }
  }
}"#;
    let config: Config = serde_json::from_str(raw).unwrap();
    let resolver = IntentToolResolver::new(&config);
    let registry = ToolRegistry::with_defaults();
    let mcp = resolved_mcp("github");

    assert!(resolver.validate_with_mcp(&registry, Some(&mcp)).is_ok());
}

#[test]
fn intent_validation_rejects_unknown_mcp_tool_when_server_tools_are_loaded() {
    let raw = r#"{
  "intentRouter": {
    "enabled": true,
    "defaultProfile": "default",
    "profiles": {
      "default": {
        "coreTools": ["read_file"],
        "intentTools": {
          "Chat": { "inheritBase": false, "tools": [] },
          "Unknown": ["github__missing_tool"]
        }
      }
    }
  }
}"#;
    let config: Config = serde_json::from_str(raw).unwrap();
    let resolver = IntentToolResolver::new(&config);
    let mut registry = ToolRegistry::with_defaults();
    let before = registry.tool_names().len();

    struct FakeGithubTool;
    #[async_trait::async_trait]
    impl blockcell_tools::Tool for FakeGithubTool {
        fn schema(&self) -> blockcell_tools::ToolSchema {
            blockcell_tools::ToolSchema {
                name: "github__search_repositories",
                description: "fake",
                parameters: serde_json::json!({"type":"object"}),
            }
        }
        fn validate(&self, _params: &serde_json::Value) -> blockcell_core::Result<()> {
            Ok(())
        }
        async fn execute(
            &self,
            _ctx: blockcell_tools::ToolContext,
            _params: serde_json::Value,
        ) -> blockcell_core::Result<serde_json::Value> {
            Ok(serde_json::Value::Null)
        }
    }
    registry.register(std::sync::Arc::new(FakeGithubTool));
    assert_eq!(registry.tool_names().len(), before + 1);

    let mcp = resolved_mcp("github");
    let err = resolver
        .validate_with_mcp(&registry, Some(&mcp))
        .expect_err("missing github tool should be rejected once github tools are discovered");

    assert!(err.to_string().contains("github__missing_tool"));
}
