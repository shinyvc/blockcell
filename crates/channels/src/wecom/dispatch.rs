use super::*;

impl WeComChannel {
    #[allow(dead_code)]
    pub(crate) async fn process_message_json(&self, msg: &serde_json::Value) -> Result<()> {
        let msg_type = msg.get("msgtype").and_then(|v| v.as_str()).unwrap_or("");
        if msg_type != "text" {
            debug!(msg_type = %msg_type, "WeCom: skipping non-text message");
            return Ok(());
        }

        let content = msg
            .get("text")
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if content.is_empty() {
            return Ok(());
        }

        let from_user = msg
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !self.is_allowed(&from_user) {
            debug!(from_user = %from_user, "WeCom: user not in allowlist");
            return Ok(());
        }

        let to_party = msg
            .get("toparty")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let msg_id = msg
            .get("msgid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let inbound = InboundMessage {
            channel: "wecom".to_string(),
            account_id: wecom_account_id(&self.config),
            sender_id: from_user.clone(),
            chat_id: if to_party.is_empty() {
                from_user
            } else {
                to_party
            },
            content,
            media: vec![],
            metadata: serde_json::json!({
                "msg_id": msg_id,
                "msg_type": msg_type,
                "mode": "polling",
            }),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        self.inbound_tx
            .send(inbound)
            .await
            .map_err(|e| Error::Channel(e.to_string()))
    }

    // ── Callback verification (for webhook mode) ──────────────────────────────

    /// Verify a WeCom callback request signature.
    /// WeCom uses SHA1(sort(token, timestamp, nonce)) for verification.
    pub fn verify_signature(token: &str, timestamp: &str, nonce: &str, signature: &str) -> bool {
        let mut parts = [token, timestamp, nonce];
        parts.sort_unstable();
        let combined = parts.join("");

        let hash = sha1_hex(combined.as_bytes());
        hash == signature
    }

    pub async fn run_loop(self: Arc<Self>, shutdown: tokio::sync::broadcast::Receiver<()>) {
        if !self.config.channels.wecom.enabled {
            info!("WeCom channel disabled");
            return;
        }

        let mode = self.config.channels.wecom.mode.trim().to_lowercase();
        info!(
            mode = %mode,
            corp_id = %self.config.channels.wecom.corp_id,
            agent_id = self.config.channels.wecom.agent_id,
            bot_id = %self.config.channels.wecom.bot_id,
            ws_url = %self.ws_url(),
            "WeCom run_loop entered"
        );
        if mode == "long_connection" || mode == "long-connection" || mode == "stream" {
            if self.config.channels.wecom.bot_id.trim().is_empty()
                || self.config.channels.wecom.bot_secret.trim().is_empty()
            {
                warn!("WeCom long_connection requires bot_id and bot_secret");
                return;
            }
            self.run_long_connection(shutdown).await;
            return;
        }

        if self.config.channels.wecom.corp_id.is_empty() {
            warn!("WeCom corp_id not configured");
            return;
        }

        if self.config.channels.wecom.corp_secret.is_empty() {
            warn!("WeCom corp_secret not configured");
            return;
        }

        match self.get_access_token().await {
            Ok(_) => info!("WeCom access token obtained successfully"),
            Err(e) => {
                error!(error = %e.to_string(), "WeCom: failed to get access token, channel will not start");
                return;
            }
        }

        self.run_polling(shutdown).await;
    }
}
