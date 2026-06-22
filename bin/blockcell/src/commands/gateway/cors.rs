//! 构建 API / WebUI 的 CORS 层。从 `gateway.rs` 抽离，行为保持不变。

use blockcell_core::Config;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// 构建 API CORS 层。
///
/// 如果配置了 `gateway.allowed_origins`，使用配置的源列表；
/// 否则回退到宽松策略（适用于本地/信任网络环境）。
pub(super) fn build_api_cors_layer(config: &Config) -> CorsLayer {
    let origins = &config.gateway.allowed_origins;
    if origins.is_empty() {
        // 未配置 allowed_origins，使用宽松策略减少用户配置负担
        CorsLayer::permissive().allow_credentials(false)
    } else {
        // 使用配置的 allowed_origins 列表
        // axum::http 重导出了 http crate 的类型
        let origin_list: Vec<axum::http::HeaderValue> =
            origins.iter().filter_map(|o| o.parse().ok()).collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origin_list))
            .allow_credentials(false)
    }
}

/// 构建 WebUI CORS 层。
///
/// 如果配置了 `gateway.allowed_origins`，使用配置的源列表；
/// 否则回退到宽松策略（WebUI 通常在同机或内网访问）。
pub(super) fn build_webui_cors_layer(config: &Config) -> CorsLayer {
    // WebUI 与 API 共用相同的 CORS 策略
    build_api_cors_layer(config)
}
