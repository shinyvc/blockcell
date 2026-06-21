use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct SteeringSessionKey {
    pub agent_id: String,
    pub chat_id: String,
}

pub type SteeringRegistry = Arc<Mutex<HashMap<SteeringSessionKey, SteeringSender>>>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SteeringMessage {
    pub content: String,
    pub channel: String,
    pub chat_id: String,
}

pub struct SteeringChannel {
    rx: mpsc::Receiver<SteeringMessage>,
}

#[derive(Clone)]
pub struct SteeringSender {
    tx: mpsc::Sender<SteeringMessage>,
}

impl SteeringChannel {
    pub fn new(buffer_size: usize) -> (Self, SteeringSender) {
        let (tx, rx) = mpsc::channel(buffer_size);
        (Self { rx }, SteeringSender { tx })
    }

    pub fn try_recv(&mut self) -> Option<SteeringMessage> {
        self.rx.try_recv().ok()
    }

    pub fn drain(&mut self) -> Vec<SteeringMessage> {
        let mut messages = Vec::new();
        while let Ok(message) = self.rx.try_recv() {
            messages.push(message);
        }
        messages
    }

    pub fn has_pending(&self) -> bool {
        !self.rx.is_empty()
    }
}

impl SteeringSender {
    pub async fn send(
        &self,
        message: SteeringMessage,
    ) -> std::result::Result<(), mpsc::error::SendError<SteeringMessage>> {
        self.tx.send(message).await
    }

    pub fn try_send(
        &self,
        message: SteeringMessage,
    ) -> std::result::Result<(), mpsc::error::TrySendError<SteeringMessage>> {
        self.tx.try_send(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(content: &str) -> SteeringMessage {
        SteeringMessage {
            content: content.to_string(),
            channel: "ws".to_string(),
            chat_id: "chat-1".to_string(),
        }
    }

    #[test]
    fn drain_returns_pending_messages_in_send_order() {
        let (mut channel, sender) = SteeringChannel::new(4);

        sender.try_send(message("first")).expect("send first");
        sender.try_send(message("second")).expect("send second");

        assert!(channel.has_pending());
        let drained = channel.drain();

        assert_eq!(drained, vec![message("first"), message("second")]);
        assert!(!channel.has_pending());
        assert!(channel.drain().is_empty());
    }

    #[test]
    fn try_recv_returns_none_when_empty() {
        let (mut channel, _sender) = SteeringChannel::new(1);

        assert_eq!(channel.try_recv(), None);
        assert!(!channel.has_pending());
    }

    #[tokio::test]
    async fn send_reports_closed_channel() {
        let (channel, sender) = SteeringChannel::new(1);
        drop(channel);

        let result = sender.send(message("closed")).await;

        assert!(result.is_err());
    }
}
