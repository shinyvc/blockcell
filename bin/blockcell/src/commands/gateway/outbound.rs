use super::*;
// ---------------------------------------------------------------------------
// Outbound → WebSocket broadcast bridge
// ---------------------------------------------------------------------------

/// Dispatches runtime outbound messages to their target non-WebSocket channels.
///
/// WebSocket clients receive runtime events through `event_tx`, where each
/// connection applies its own session filter. Mirroring non-WS outbound traffic
/// into `ws_broadcast` would leak external-channel conversations to WebUI users.
pub(super) async fn outbound_to_ws_bridge(
    mut outbound_rx: mpsc::Receiver<blockcell_core::OutboundMessage>,
    _ws_broadcast: broadcast::Sender<String>,
    channel_manager: Arc<ChannelManager>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            msg = outbound_rx.recv() => {
                let Some(msg) = msg else { break };

                // Also dispatch to external channels (telegram, slack, etc.)
                if msg.channel != "ws" && msg.channel != "cli" && msg.channel != "http" {
                    if let Err(e) = channel_manager.dispatch_outbound_msg(&msg).await {
                        error!(error = %e, channel = %msg.channel, "Failed to dispatch outbound message");
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                debug!("outbound_to_ws_bridge received shutdown signal");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::{broadcast, mpsc};

    #[tokio::test]
    async fn outbound_bridge_does_not_broadcast_non_ws_messages_to_websocket_clients() {
        let (outbound_tx, outbound_rx) = mpsc::channel(4);
        let (ws_broadcast, mut ws_rx) = broadcast::channel(4);
        let channel_manager = Arc::new(ChannelManager::new(
            Config::default(),
            Paths::with_base(std::env::temp_dir().join("blockcell-outbound-bridge-test")),
            mpsc::channel(1).0,
        ));
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let handle = tokio::spawn(outbound_to_ws_bridge(
            outbound_rx,
            ws_broadcast,
            channel_manager,
            shutdown_rx,
        ));

        outbound_tx
            .send(OutboundMessage::new("telegram", "chat-a", "private reply"))
            .await
            .expect("outbound bridge should accept test message");

        let leaked = tokio::time::timeout(std::time::Duration::from_millis(50), ws_rx.recv()).await;
        assert!(
            leaked.is_err(),
            "non-ws outbound messages must not be visible to websocket clients"
        );

        let _ = shutdown_tx.send(());
        handle
            .await
            .expect("outbound bridge task should exit cleanly");
    }
}
