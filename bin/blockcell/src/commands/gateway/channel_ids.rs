//! 渠道账号 ID 枚举/校验与消息路由 agent 解析相关的纯函数。
//!
//! 从 `gateway.rs` 抽离：列举/过滤各渠道已配置账号、校验 channelAccountOwners
//! 绑定、判断内部渠道、以及把入站消息解析/标注到具体 agent。这些函数仅依赖
//! `Config` / `InboundMessage`，不触及 GatewayState，行为保持不变。

use blockcell_core::{Config, InboundMessage};

pub(super) const EXTERNAL_CHANNELS: [&str; 11] = [
    "telegram", "whatsapp", "feishu", "slack", "discord", "dingtalk", "wecom", "lark", "qq",
    "napcat", "weixin",
];

pub(super) fn known_channel_account_ids(config: &Config, channel: &str) -> Vec<String> {
    let mut ids = match channel {
        "telegram" => config
            .channels
            .telegram
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "whatsapp" => config
            .channels
            .whatsapp
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "feishu" => config
            .channels
            .feishu
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "slack" => config
            .channels
            .slack
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "discord" => config
            .channels
            .discord
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "dingtalk" => config
            .channels
            .dingtalk
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "wecom" => config
            .channels
            .wecom
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "lark" => config
            .channels
            .lark
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "weixin" => config
            .channels
            .weixin
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "qq" => config
            .channels
            .qq
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        "napcat" => config
            .channels
            .napcat
            .accounts
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    ids.sort();
    ids
}

pub(super) fn enabled_channel_account_ids(config: &Config, channel: &str) -> Vec<String> {
    let mut ids = match channel {
        "telegram" => config
            .channels
            .telegram
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.token.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "whatsapp" => config
            .channels
            .whatsapp
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.bridge_url.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "feishu" => config
            .channels
            .feishu
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.app_id.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "slack" => config
            .channels
            .slack
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.bot_token.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "discord" => config
            .channels
            .discord
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.bot_token.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "dingtalk" => config
            .channels
            .dingtalk
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.app_key.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "wecom" => config
            .channels
            .wecom
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.corp_id.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "lark" => config
            .channels
            .lark
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.app_id.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "weixin" => config
            .channels
            .weixin
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.token.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "qq" => config
            .channels
            .qq
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled && !account.app_id.trim().is_empty())
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        "napcat" => config
            .channels
            .napcat
            .accounts
            .iter()
            .filter(|(_, account)| {
                account.enabled
                    && account
                        .ws_url
                        .as_ref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false)
            })
            .map(|(account_id, _)| account_id.clone())
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    ids.sort();
    ids
}

pub(super) fn validate_channel_owner_bindings(config: &Config) -> anyhow::Result<()> {
    for channel in EXTERNAL_CHANNELS {
        let account_owner_bindings = config.channel_account_owners.get(channel);
        let known_account_ids = known_channel_account_ids(config, channel);

        if let Some(bindings) = account_owner_bindings {
            for (account_id, owner) in bindings {
                let account_id = account_id.trim();
                let owner = owner.trim();
                if account_id.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Channel '{}' has an empty account id in channelAccountOwners.",
                        channel
                    ));
                }
                if owner.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Channel '{}' account '{}' has a blank owner agent.",
                        channel,
                        account_id
                    ));
                }
                if !known_account_ids.iter().any(|id| id == account_id) {
                    return Err(anyhow::anyhow!(
                        "Channel '{}' account '{}' is not defined under channels.{}.accounts.",
                        channel,
                        account_id,
                        channel
                    ));
                }
                if !config.agent_exists(owner) {
                    return Err(anyhow::anyhow!(
                        "Channel '{}' account owner '{}' does not exist in agents.list.",
                        channel,
                        owner
                    ));
                }
            }
        }

        if !config.is_external_channel_enabled(channel) {
            continue;
        }

        if let Some(owner) = config.resolve_channel_owner(channel) {
            if !config.agent_exists(owner) {
                return Err(anyhow::anyhow!(
                    "Channel '{}' owner '{}' does not exist in agents.list.",
                    channel,
                    owner
                ));
            }
            continue;
        }

        let enabled_account_ids = enabled_channel_account_ids(config, channel);
        if enabled_account_ids.is_empty() {
            return Err(anyhow::anyhow!(
                "Channel '{}' is enabled but has no owner agent. Set channelOwners.{} in config.",
                channel,
                channel
            ));
        }

        for account_id in enabled_account_ids {
            if config
                .resolve_channel_account_owner(channel, &account_id)
                .is_none()
            {
                return Err(anyhow::anyhow!(
                    "Channel '{}' is enabled but missing owner binding for enabled account '{}'. Set channelAccountOwners.{}.{} or channelOwners.{}.",
                    channel,
                    account_id,
                    channel,
                    account_id,
                    channel
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn is_internal_channel(channel: &str) -> bool {
    matches!(
        channel,
        "ws" | "cli" | "cron" | "system" | "subagent" | "heartbeat" | "ghost"
    )
}

pub(super) fn metadata_route_agent_id(msg: &InboundMessage) -> Option<String> {
    msg.metadata
        .get("route_agent_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn resolve_runtime_agent_id(config: &Config, msg: &InboundMessage) -> Option<String> {
    if let Some(agent_id) = metadata_route_agent_id(msg) {
        return config.agent_exists(&agent_id).then_some(agent_id);
    }

    if is_internal_channel(&msg.channel) {
        return Some("default".to_string());
    }

    let owner = config.resolve_effective_channel_owner(&msg.channel, msg.account_id.as_deref())?;
    config.agent_exists(owner).then(|| owner.to_string())
}

pub(super) fn resolve_requested_agent_id(
    config: &Config,
    requested: Option<&str>,
) -> std::result::Result<String, String> {
    let agent_id = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("default");

    if config.agent_exists(agent_id) {
        Ok(agent_id.to_string())
    } else {
        Err(format!("Unknown agent '{}'", agent_id))
    }
}

pub(super) fn with_route_agent_id(mut msg: InboundMessage, agent_id: &str) -> InboundMessage {
    let mut metadata = if msg.metadata.is_object() {
        msg.metadata
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = metadata.as_object_mut() {
        obj.insert("route_agent_id".to_string(), serde_json::json!(agent_id));
        if !is_internal_channel(&msg.channel) {
            obj.entry("route_match_level".to_string())
                .or_insert_with(|| serde_json::json!("channel_owner"));
        }
    }

    msg.metadata = metadata;
    msg
}
