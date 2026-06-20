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
