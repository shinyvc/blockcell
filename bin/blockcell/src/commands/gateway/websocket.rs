use super::*;
use crate::commands::gateway::chat::assign_session_id;
use crate::commands::slash_commands::{CommandContext, CommandResult, SLASH_COMMAND_HANDLER};
// ---------------------------------------------------------------------------
// P0: WebSocket with structured protocol
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
struct WsSessionScope {
    agent_id: String,
    chat_id: String,
}

fn ws_event_visible_to_connection(
    subscriptions: &std::collections::HashSet<WsSessionScope>,
    connection_id: &str,
    msg: &str,
) -> bool {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(msg) else {
        return false;
    };

    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if matches!(
        event_type,
        "skills_updated"
            | "evolution_triggered"
            | "evolution_resumed"
            | "evolution_stopped"
            | "evolution_deleted"
    ) {
        return true;
    }

    if event.get("channel").and_then(|v| v.as_str()) != Some("ws") {
        return false;
    }

    let Some(chat_id) = event.get("chat_id").and_then(|v| v.as_str()) else {
        return false;
    };
    if chat_id.is_empty() {
        return false;
    }

    let agent_id = event
        .get("agent_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    if !subscriptions.contains(&WsSessionScope {
        agent_id: agent_id.to_string(),
        chat_id: chat_id.to_string(),
    }) {
        return false;
    }

    if let Some(expected_connection_id) = event.get("ws_connection_id").and_then(|v| v.as_str()) {
        return expected_connection_id == connection_id;
    }

    true
}

fn ws_confirm_response_allowed(
    subscriptions: &std::collections::HashSet<WsSessionScope>,
    connection_id: &str,
    pending: &PendingWsConfirmScope,
) -> bool {
    let subscribed = subscriptions.contains(&WsSessionScope {
        agent_id: pending.agent_id.clone(),
        chat_id: pending.chat_id.clone(),
    });
    if !subscribed {
        return false;
    }

    match pending.ws_connection_id.as_deref() {
        Some(expected) => expected == connection_id,
        None => false,
    }
}

pub(super) async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<GatewayState>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    // Validate token inside the WS handler so we can close with code 4401
    // instead of rejecting the HTTP upgrade with 401 (which gives client code 1006).
    let token_valid = match &state.api_token {
        Some(t) if !t.is_empty() => {
            let auth_header = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok());
            let from_header = match auth_header {
                Some(h) if h.starts_with("Bearer ") => secure_eq(&h[7..], t.as_str()),
                _ => false,
            };
            let from_query = token_from_query(&req)
                .map(|v| secure_eq(&v, t.as_str()))
                .unwrap_or(false);
            from_header || from_query
        }
        _ => true, // no token configured → open access
    };

    ws.on_upgrade(move |socket| async move {
        if !token_valid {
            let mut socket = socket;
            let _ = socket
                .send(WsMessage::Close(Some(axum::extract::ws::CloseFrame {
                    code: 4401,
                    reason: std::borrow::Cow::Borrowed("Unauthorized"),
                })))
                .await;
            return;
        }
        handle_ws_connection(socket, state).await;
    })
}

pub(super) async fn handle_ws_connection(socket: WebSocket, state: GatewayState) {
    info!("WebSocket client connected");

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut broadcast_rx = state.ws_broadcast.subscribe();
    let subscriptions = Arc::new(Mutex::new(std::collections::HashSet::new()));
    let (direct_tx, mut direct_rx) = mpsc::channel::<String>(16);
    let connection_id = format!("ws_{}", uuid::Uuid::new_v4().simple());

    use futures::SinkExt;
    use futures::StreamExt;

    // Task: forward broadcast events to this WS client
    let send_subscriptions = Arc::clone(&subscriptions);
    let send_connection_id = connection_id.clone();
    let send_task = tokio::spawn(async move {
        let mut direct_open = true;
        loop {
            tokio::select! {
                direct = direct_rx.recv(), if direct_open => {
                    let Some(msg) = direct else {
                        direct_open = false;
                        continue;
                    };
                    if ws_sender.send(WsMessage::Text(msg)).await.is_err() {
                        break;
                    }
                }
                received = broadcast_rx.recv() => {
                    let Ok(msg) = received else {
                        break;
                    };
                    let visible = {
                        let subscriptions = send_subscriptions.lock().await;
                        ws_event_visible_to_connection(&subscriptions, &send_connection_id, &msg)
                    };
                    if !visible {
                        continue;
                    }
                    if ws_sender.send(WsMessage::Text(msg)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Task: receive messages from this WS client
    let inbound_tx = state.inbound_tx.clone();
    let ws_broadcast = state.ws_broadcast.clone();

    while let Some(msg) = ws_receiver.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "WebSocket receive error");
                break;
            }
        };

        match msg {
            WsMessage::Text(text) => {
                // Parse structured message
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                    let msg_type = parsed
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("chat");

                    match msg_type {
                        "chat" => {
                            let mut content = parsed
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let client_chat_id = parsed
                                .get("chat_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let media: Vec<String> = parsed
                                .get("media")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();

                            let requested_agent_id =
                                parsed.get("agent_id").and_then(|v| v.as_str());
                            let resolved_agent_id = match requested_agent_id {
                                Some(requested) => {
                                    match resolve_requested_agent_id(&state.config, Some(requested))
                                    {
                                        Ok(agent_id) => agent_id,
                                        Err(err) => {
                                            let _ = direct_tx
                                                .send(
                                                    serde_json::json!({
                                                        "type": "error",
                                                        "channel": "ws",
                                                        "client_chat_id": client_chat_id,
                                                        "chat_id": client_chat_id,
                                                        "message": err,
                                                    })
                                                    .to_string(),
                                                )
                                                .await;
                                            continue;
                                        }
                                    }
                                }
                                None => "default".to_string(),
                            };

                            let chat_id = assign_session_id(&client_chat_id, &resolved_agent_id);
                            {
                                let mut subscriptions = subscriptions.lock().await;
                                subscriptions.insert(WsSessionScope {
                                    agent_id: resolved_agent_id.clone(),
                                    chat_id: chat_id.clone(),
                                });
                            }

                            let _ = ws_broadcast.send(
                                WsEvent::SessionBound {
                                    channel: "ws".to_string(),
                                    client_chat_id: client_chat_id.clone(),
                                    chat_id: chat_id.clone(),
                                    agent_id: resolved_agent_id.clone(),
                                }
                                .to_json(),
                            );

                            // 斜杠命令拦截：在创建 InboundMessage 之前检查
                            let mut ws_metadata = serde_json::json!({
                                "ws_connection_id": connection_id.clone(),
                            });
                            if content.starts_with('/') {
                                let session_key = format!("ws:{}", chat_id);
                                let ctx = CommandContext::for_websocket(
                                    state.paths.clone(),
                                    state.task_manager.clone(),
                                    state.checkpoint_manager.clone(),
                                    chat_id.clone(),
                                )
                                .with_clear_callback(
                                    super::create_session_clear_callback(
                                        state.response_caches.clone(),
                                        resolved_agent_id.clone(),
                                        session_key,
                                    ),
                                );

                                match SLASH_COMMAND_HANDLER.try_handle(&content, &ctx).await {
                                    CommandResult::Handled(response) => {
                                        // 复用 message_done 事件（前端已支持）
                                        let event = serde_json::json!({
                                            "type": "message_done",
                                            "channel": "ws",
                                            "agent_id": resolved_agent_id,
                                            "chat_id": chat_id,
                                            "content": response.content,
                                            "is_markdown": response.is_markdown,
                                            "task_id": "",
                                        });
                                        let _ = ws_broadcast.send(event.to_string());
                                        continue; // 不转发给 AgentRuntime
                                    }
                                    CommandResult::NotACommand => {
                                        // 非斜杠命令，继续正常消息处理流程
                                    }
                                    CommandResult::PermissionDenied(msg) => {
                                        let _ = ws_broadcast.send(
                                            serde_json::json!({
                                                "type": "error",
                                                "channel": "ws",
                                                "agent_id": resolved_agent_id,
                                                "chat_id": chat_id,
                                                "message": format!("权限不足: {}", msg),
                                            })
                                            .to_string(),
                                        );
                                        continue;
                                    }
                                    CommandResult::Error(e) => {
                                        let _ = ws_broadcast.send(
                                            serde_json::json!({
                                                "type": "error",
                                                "channel": "ws",
                                                "agent_id": resolved_agent_id,
                                                "chat_id": chat_id,
                                                "message": format!("命令执行错误: {}", e),
                                            })
                                            .to_string(),
                                        );
                                        continue;
                                    }
                                    CommandResult::ExitRequested => {
                                        // /quit 和 /exit 在 WebSocket 模式下不应该到达这里
                                        // 因为渠道限制会在 try_handle 中处理
                                        let _ = ws_broadcast.send(
                                            serde_json::json!({
                                                "type": "error",
                                                "channel": "ws",
                                                "agent_id": resolved_agent_id,
                                                "chat_id": chat_id,
                                                "message": "此命令仅在 CLI 模式可用",
                                            })
                                            .to_string(),
                                        );
                                        continue;
                                    }
                                    CommandResult::ForwardToRuntime {
                                        transformed_content,
                                        original_command,
                                    } => {
                                        // 命令需要转发给 AgentRuntime（如 /learn, /cancel-task, /resume）
                                        tracing::info!(
                                            command = %original_command,
                                            "Forwarding command to AgentRuntime"
                                        );
                                        // 使用转换后的内容替代原始内容
                                        content = transformed_content;
                                        // 标记来源为斜杠命令，runtime 据此验证授权
                                        ws_metadata = serde_json::json!({
                                            "ws_connection_id": connection_id.clone(),
                                            "source": "slash_command",
                                            "original_command": original_command
                                        });
                                        // 继续正常流程，转发给 AgentRuntime
                                    }
                                }
                            }

                            let inbound = InboundMessage {
                                channel: "ws".to_string(),
                                account_id: None,
                                sender_id: "user".to_string(),
                                chat_id: chat_id.clone(),
                                content,
                                media,
                                metadata: ws_metadata,
                                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            };

                            let inbound = with_route_agent_id(inbound, &resolved_agent_id);

                            if let Err(e) = inbound_tx.send(inbound).await {
                                let _ = ws_broadcast.send(
                                    WsEvent::error(chat_id.clone(), format!("{}", e)).to_json(),
                                );
                                break;
                            }
                        }
                        "confirm_response" => {
                            let request_id = parsed
                                .get("request_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let approved = parsed
                                .get("approved")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if !request_id.is_empty() {
                                let mut map = state.pending_confirms.lock().await;
                                let allowed = if let Some(pending) = map.get(&request_id) {
                                    let subscriptions = subscriptions.lock().await;
                                    ws_confirm_response_allowed(
                                        &subscriptions,
                                        &connection_id,
                                        &pending.scope,
                                    )
                                } else {
                                    false
                                };
                                if allowed {
                                    if let Some(pending) = map.remove(&request_id) {
                                        let _ = pending.response_tx.send(approved);
                                        debug!(request_id = %request_id, approved, "Confirm response routed");
                                    }
                                } else {
                                    warn!(request_id = %request_id, "Rejected unauthorized confirm response");
                                }
                            }
                        }
                        "cancel" => {
                            let chat_id = parsed
                                .get("chat_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("default")
                                .to_string();
                            debug!(chat_id = %chat_id, "Received cancel via WS");

                            let inbound = InboundMessage {
                                channel: "ws".to_string(),
                                account_id: None,
                                sender_id: "user".to_string(),
                                chat_id: chat_id.clone(),
                                content: "[cancel]".to_string(),
                                media: vec![],
                                metadata: serde_json::json!({
                                    "cancel": true,
                                }),
                                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            };

                            let inbound = match parsed.get("agent_id").and_then(|v| v.as_str()) {
                                Some(requested) => {
                                    match resolve_requested_agent_id(&state.config, Some(requested))
                                    {
                                        Ok(agent_id) => with_route_agent_id(inbound, &agent_id),
                                        Err(err) => {
                                            let _ = direct_tx
                                                .send(
                                                    serde_json::json!({
                                                        "type": "error",
                                                        "channel": "ws",
                                                        "chat_id": chat_id,
                                                        "message": err,
                                                    })
                                                    .to_string(),
                                                )
                                                .await;
                                            continue;
                                        }
                                    }
                                }
                                None => inbound,
                            };

                            if let Err(e) = inbound_tx.send(inbound).await {
                                let _ = ws_broadcast
                                    .send(WsEvent::error(chat_id, format!("{}", e)).to_json());
                            }
                        }
                        _ => {
                            // Fallback: treat as plain chat
                            {
                                let mut subscriptions = subscriptions.lock().await;
                                subscriptions.insert(WsSessionScope {
                                    agent_id: "default".to_string(),
                                    chat_id: "default".to_string(),
                                });
                            }
                            let inbound = InboundMessage {
                                channel: "ws".to_string(),
                                account_id: None,
                                sender_id: "user".to_string(),
                                chat_id: "default".to_string(),
                                content: text.to_string(),
                                media: vec![],
                                metadata: serde_json::json!({
                                    "ws_connection_id": connection_id.clone(),
                                }),
                                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            };
                            let _ = inbound_tx.send(inbound).await;
                        }
                    }
                } else {
                    // Plain text fallback
                    {
                        let mut subscriptions = subscriptions.lock().await;
                        subscriptions.insert(WsSessionScope {
                            agent_id: "default".to_string(),
                            chat_id: "default".to_string(),
                        });
                    }
                    let inbound = InboundMessage {
                        channel: "ws".to_string(),
                        account_id: None,
                        sender_id: "user".to_string(),
                        chat_id: "default".to_string(),
                        content: text.to_string(),
                        media: vec![],
                        metadata: serde_json::json!({
                            "ws_connection_id": connection_id.clone(),
                        }),
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    };
                    let _ = inbound_tx.send(inbound).await;
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }

    send_task.abort();
    info!("WebSocket client disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn scope(agent_id: &str, chat_id: &str) -> WsSessionScope {
        WsSessionScope {
            agent_id: agent_id.to_string(),
            chat_id: chat_id.to_string(),
        }
    }

    #[test]
    fn ws_event_filter_allows_only_subscribed_session_events() {
        let subscriptions = HashSet::from([scope("default", "chat-a")]);

        let own = serde_json::json!({
            "type": "message_done",
            "channel": "ws",
            "agent_id": "default",
            "chat_id": "chat-a",
            "content": "visible",
        })
        .to_string();
        assert!(ws_event_visible_to_connection(
            &subscriptions,
            "connection-a",
            &own
        ));

        let other_chat = serde_json::json!({
            "type": "message_done",
            "channel": "ws",
            "agent_id": "default",
            "chat_id": "chat-b",
            "content": "hidden",
        })
        .to_string();
        assert!(!ws_event_visible_to_connection(
            &subscriptions,
            "connection-a",
            &other_chat
        ));

        let other_agent = serde_json::json!({
            "type": "message_done",
            "channel": "ws",
            "agent_id": "ops",
            "chat_id": "chat-a",
            "content": "hidden",
        })
        .to_string();
        assert!(!ws_event_visible_to_connection(
            &subscriptions,
            "connection-a",
            &other_agent
        ));
    }

    #[test]
    fn ws_event_filter_rejects_non_ws_or_unscoped_session_events_even_when_chat_id_matches() {
        let subscriptions = HashSet::from([scope("default", "chat-a")]);

        let external = serde_json::json!({
            "type": "token",
            "channel": "telegram",
            "agent_id": "default",
            "chat_id": "chat-a",
            "delta": "hidden",
        })
        .to_string();
        assert!(!ws_event_visible_to_connection(
            &subscriptions,
            "connection-a",
            &external
        ));

        let missing_channel = serde_json::json!({
            "type": "token",
            "agent_id": "default",
            "chat_id": "chat-a",
            "delta": "hidden",
        })
        .to_string();
        assert!(!ws_event_visible_to_connection(
            &subscriptions,
            "connection-a",
            &missing_channel
        ));
    }

    #[test]
    fn ws_event_filter_rejects_prebind_error_without_session_subscription() {
        let subscriptions = HashSet::new();
        let event = serde_json::json!({
            "type": "error",
            "channel": "ws",
            "client_chat_id": "draft-session",
            "chat_id": "draft-session",
            "message": "Unknown agent 'missing'",
        })
        .to_string();

        assert!(!ws_event_visible_to_connection(
            &subscriptions,
            "connection-a",
            &event
        ));
    }

    #[test]
    fn ws_event_filter_keeps_global_dashboard_refresh_events() {
        let subscriptions = HashSet::new();
        let event = serde_json::json!({
            "type": "skills_updated",
            "new_skills": ["demo"],
        })
        .to_string();

        assert!(ws_event_visible_to_connection(
            &subscriptions,
            "connection-a",
            &event
        ));
    }

    #[test]
    fn ws_event_filter_restricts_connection_scoped_confirm_requests() {
        let subscriptions = HashSet::from([scope("default", "chat-a")]);
        let event = serde_json::json!({
            "type": "confirm_request",
            "channel": "ws",
            "agent_id": "default",
            "chat_id": "chat-a",
            "ws_connection_id": "origin-connection",
            "request_id": "confirm_123",
        })
        .to_string();

        assert!(!ws_event_visible_to_connection(
            &subscriptions,
            "other-connection",
            &event
        ));
        assert!(ws_event_visible_to_connection(
            &subscriptions,
            "origin-connection",
            &event
        ));
    }

    #[test]
    fn ws_confirm_response_requires_matching_connection_id() {
        let subscriptions = HashSet::from([scope("default", "chat-a")]);
        let pending = PendingWsConfirmScope {
            agent_id: "default".to_string(),
            chat_id: "chat-a".to_string(),
            ws_connection_id: Some("origin-connection".to_string()),
        };

        assert!(!ws_confirm_response_allowed(
            &subscriptions,
            "other-connection",
            &pending
        ));
        assert!(ws_confirm_response_allowed(
            &subscriptions,
            "origin-connection",
            &pending
        ));
    }

    #[test]
    fn ws_confirm_response_rejects_ws_pending_without_connection_id() {
        let pending = PendingWsConfirmScope {
            agent_id: "default".to_string(),
            chat_id: "chat-a".to_string(),
            ws_connection_id: None,
        };

        assert!(!ws_confirm_response_allowed(
            &HashSet::new(),
            "any-connection",
            &pending
        ));
        assert!(!ws_confirm_response_allowed(
            &HashSet::from([scope("default", "chat-a")]),
            "any-connection",
            &pending
        ));
    }
}
