//! 自升级（auto-upgrade）配置类型。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoUpgradeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_upgrade_channel")]
    pub channel: String,
    #[serde(default = "default_manifest_url")]
    pub manifest_url: String,
    #[serde(default = "default_require_signature")]
    pub require_signature: bool,
    #[serde(default)]
    pub maintenance_window: String,
}

impl Default for AutoUpgradeConfig {
    fn default() -> Self {
        // 与 serde 字段默认保持一致；尤其是 require_signature 必须默认开启，
        // 否则当配置中缺失整个 autoUpgrade 段时会退化为派生默认的 false。
        Self {
            enabled: false,
            channel: default_upgrade_channel(),
            manifest_url: default_manifest_url(),
            require_signature: default_require_signature(),
            maintenance_window: String::new(),
        }
    }
}

fn default_upgrade_channel() -> String {
    "stable".to_string()
}

/// 默认要求校验自升级产物的 ed25519 签名（fail-closed）。
/// 仅靠同源 manifest 的 SHA256 无法防御 manifest 被篡改/中间人，
/// 因此默认开启签名校验；如需关闭须在配置中显式 `requireSignature: false`。
fn default_require_signature() -> bool {
    true
}

fn default_manifest_url() -> String {
    "https://github.com/blockcell-labs/blockcell/releases/latest/download/manifest.json".to_string()
}
