use tokio::sync::broadcast;

use crate::types::ServerState;

#[derive(Debug, Clone)]
pub enum SseEvent {
    FullState(Vec<ServerState>),
    Update(ServerState),
    ConfigReloaded { server_id: String, message: String },
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<SseEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn send(&self, event: SseEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SseEvent> {
        self.tx.subscribe()
    }
}
