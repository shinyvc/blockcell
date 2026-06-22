//! 日志配置类型。

use serde::{Deserialize, Serialize};

/// 日志配置。
/// 控制日志输出方式和等级。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogConfig {
    /// 日志等级: trace, debug, info, warn, error, off。默认: info
    #[serde(default = "default_log_level")]
    pub level: String,
    /// 是否输出到文件。默认: false
    #[serde(default)]
    pub file_enabled: bool,
    /// 是否输出到控制台。默认: true
    #[serde(default = "super::default_true")]
    pub console_enabled: bool,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_enabled: false,
            console_enabled: true,
        }
    }
}
