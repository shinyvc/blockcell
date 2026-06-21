use super::*;

impl WeComChannel {
    pub fn new(config: Config, inbound_tx: mpsc::Sender<InboundMessage>) -> Self {
        // 从 BLOCKCELL_WORKSPACE 环境变量读取 media 目录，支持自定义 workspace
        let media_dir = std::env::var("BLOCKCELL_WORKSPACE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .map(|h| h.join(".blockcell").join("workspace"))
                    .unwrap_or_else(|| std::path::PathBuf::from(".blockcell/workspace"))
            })
            .join("media");
        let _ = std::fs::create_dir_all(&media_dir);
        Self {
            config,
            client: shared_client(),
            inbound_tx,
            token_cache: Arc::new(tokio::sync::Mutex::new(CachedToken::default())),
            media_dir,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn is_allowed(&self, user_id: &str) -> bool {
        let allow_from = &self.config.channels.wecom.allow_from;
        if allow_from.is_empty() {
            return true;
        }
        allow_from.iter().any(|a| a == user_id)
    }

    pub async fn get_access_token(&self) -> Result<String> {
        let token = fetch_access_token_static(&self.client, &self.config).await?;
        let mut cache = self.token_cache.lock().await;
        cache.token = token.clone();
        cache.expires_at = chrono::Utc::now().timestamp() + 7200;
        Ok(token)
    }

    // ── Polling mode ──────────────────────────────────────────────────────────

    /// Poll for new messages via WeCom message API.
    /// WeCom doesn't have a direct "get messages" polling API for app messages;
    /// instead we use the appchat message list or rely on callback.
    /// This implementation uses a simple polling approach via message statistics.
    pub(crate) async fn run_polling(&self, mut shutdown: tokio::sync::broadcast::Receiver<()>) {
        let poll_interval =
            Duration::from_secs(self.config.channels.wecom.poll_interval_secs.max(5) as u64);

        info!(
            interval_secs = poll_interval.as_secs(),
            "WeCom channel started (polling mode)"
        );

        // Only warn if callback credentials are missing — if they're configured,
        // the user is using webhook mode via gateway and polling is just a heartbeat.
        if self.config.channels.wecom.callback_token.is_empty()
            || self.config.channels.wecom.encoding_aes_key.is_empty()
        {
            warn!(
                "WeCom polling mode: WeCom requires a callback URL for real-time message reception. \
                 Configure 'callback_token' and 'encoding_aes_key' and set up your server's \
                 callback URL in the WeCom admin console for full functionality. \
                 Polling mode will only process messages sent via the agent's send_message API."
            );
        }

        loop {
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {
                    // In polling mode, we can check for pending messages
                    // via the WeCom message API if configured
                    if let Err(e) = self.poll_messages().await {
                        error!(error = %e.to_string(), "WeCom poll error");
                    }
                }
                _ = shutdown.recv() => {
                    info!("WeCom channel shutting down (polling)");
                    break;
                }
            }
        }
    }

    pub(crate) async fn poll_messages(&self) -> Result<()> {
        // WeCom does not provide a public API for polling received app messages.
        // The correct approach is to configure a callback URL in the WeCom admin
        // console. In polling mode we simply verify the token is still valid.
        let _token = self.get_access_token().await?;
        debug!(
            "WeCom token heartbeat OK (polling mode — no inbound messages without callback URL)"
        );
        Ok(())
    }
}
