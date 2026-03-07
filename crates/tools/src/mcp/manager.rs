use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{info, warn};

use blockcell_core::mcp_config::McpResolvedConfig;
use blockcell_core::{Error, Paths, Result};

use crate::mcp::client::McpClient;
use crate::mcp::provider::McpToolWrapper;
use crate::ToolRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolTarget {
    pub server_name: String,
    pub tool_name: String,
}

impl McpToolTarget {
    pub fn parse(qualified_tool_name: &str) -> Option<Self> {
        let (server_name, tool_name) = qualified_tool_name.split_once("__")?;
        let server_name = server_name.trim();
        let tool_name = tool_name.trim();
        if server_name.is_empty() || tool_name.is_empty() {
            return None;
        }
        Some(Self {
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
        })
    }
}

pub fn mcp_access_allows(
    allowed_servers: &[String],
    allowed_tools: &[String],
    qualified_tool_name: &str,
) -> bool {
    if allowed_tools.iter().any(|tool| tool == qualified_tool_name) {
        return true;
    }

    let Some(target) = McpToolTarget::parse(qualified_tool_name) else {
        return false;
    };

    allowed_servers
        .iter()
        .any(|server_name| server_name == &target.server_name)
}

pub struct McpManager {
    resolved: McpResolvedConfig,
    clients: Mutex<HashMap<String, Arc<McpClient>>>,
}

impl McpManager {
    pub async fn load(paths: &Paths) -> Result<Self> {
        let resolved = McpResolvedConfig::load_merged(paths)?;
        let manager = Self {
            resolved,
            clients: Mutex::new(HashMap::new()),
        };
        manager.start_auto_start_servers().await;
        Ok(manager)
    }

    pub fn resolved_config(&self) -> &McpResolvedConfig {
        &self.resolved
    }

    pub fn enabled_server_names(&self) -> Vec<String> {
        self.resolved
            .servers
            .iter()
            .filter_map(|(name, cfg)| cfg.enabled.then_some(name.clone()))
            .collect()
    }

    async fn start_auto_start_servers(&self) {
        for server_name in self.enabled_server_names() {
            let Some(server_cfg) = self.resolved.servers.get(&server_name) else {
                continue;
            };
            if !server_cfg.auto_start {
                continue;
            }
            if let Err(error) = self.client_for(&server_name).await {
                warn!(server = %server_name, error = %error, "Failed to auto-start MCP server");
            }
        }
    }

    pub async fn client_for(&self, server_name: &str) -> Result<Arc<McpClient>> {
        {
            let clients = self.clients.lock().await;
            if let Some(client) = clients.get(server_name) {
                return Ok(client.clone());
            }
        }

        let server_cfg =
            self.resolved.servers.get(server_name).ok_or_else(|| {
                Error::NotFound(format!("MCP server '{}' not found", server_name))
            })?;

        if !server_cfg.enabled {
            return Err(Error::Config(format!(
                "MCP server '{}' is disabled",
                server_name
            )));
        }

        info!(server = %server_name, command = %server_cfg.command, "Starting MCP server");
        let client = Arc::new(
            McpClient::start(
                server_name,
                &server_cfg.command,
                &server_cfg.args,
                &server_cfg.env,
                server_cfg.cwd.as_deref(),
                std::time::Duration::from_secs(server_cfg.startup_timeout_secs),
                std::time::Duration::from_secs(server_cfg.call_timeout_secs),
            )
            .await?,
        );

        let mut clients = self.clients.lock().await;
        Ok(clients
            .entry(server_name.to_string())
            .or_insert_with(|| client.clone())
            .clone())
    }

    pub async fn extend_registry_all(&self, registry: &mut ToolRegistry) -> Result<()> {
        let allowed_servers = self.enabled_server_names();
        self.extend_registry_for_rules(registry, &allowed_servers, &[])
            .await
    }

    pub async fn extend_registry_for_rules(
        &self,
        registry: &mut ToolRegistry,
        allowed_servers: &[String],
        allowed_tools: &[String],
    ) -> Result<()> {
        for server_name in self.enabled_server_names() {
            let client = match self.client_for(&server_name).await {
                Ok(client) => client,
                Err(error) => {
                    warn!(server = %server_name, error = %error, "Skipping unavailable MCP server");
                    continue;
                }
            };

            for tool in client.list_tools().await {
                let qualified_tool_name = format!("{}__{}", server_name, tool.name);
                if mcp_access_allows(allowed_servers, allowed_tools, &qualified_tool_name) {
                    registry.register(Arc::new(McpToolWrapper::new(
                        &server_name,
                        tool,
                        client.clone(),
                    )));
                }
            }
        }

        Ok(())
    }
}
