//! Gateway / WebUI 服务配置类型。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfig {
    #[serde(default = "default_gateway_host")]
    pub host: String,
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    #[serde(default = "default_webui_host")]
    pub webui_host: String,
    #[serde(default = "default_webui_port")]
    pub webui_port: u16,
    /// Optional public API base URL injected into WebUI at runtime.
    /// Example: "https://your-domain.example.com" or "https://your-domain.example.com/api".
    /// If not set, WebUI will default to current hostname + gateway.port.
    #[serde(default)]
    pub public_api_base: Option<String>,
    #[serde(default)]
    pub api_token: Option<String>,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// WebUI login password. If empty/None, a temporary password is printed at startup.
    #[serde(default)]
    pub webui_pass: Option<String>,
}

fn default_gateway_host() -> String {
    "localhost".to_string()
}

fn default_gateway_port() -> u16 {
    18790
}

fn default_webui_host() -> String {
    "localhost".to_string()
}

fn default_webui_port() -> u16 {
    18791
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_gateway_host(),
            port: default_gateway_port(),
            webui_host: default_webui_host(),
            webui_port: default_webui_port(),
            public_api_base: None,
            api_token: None,
            allowed_origins: vec![],
            webui_pass: None,
        }
    }
}
