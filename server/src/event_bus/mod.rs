use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// EventBus maps a key (session_id) to a broadcast::Sender. Dead senders
/// (receiver_count == 0) are lazily cleaned up on access so abandoned
/// channels don't accumulate forever.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Mutex<HashMap<String, broadcast::Sender<Value>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns a sender for the given key, creating one if absent.
    /// Removes dead entries (all receivers dropped) before returning.
    pub fn ensure_channel(&self, key: &str) -> broadcast::Sender<Value> {
        let mut map = self.inner.lock().expect("event bus lock");
        // Remove entries where all receivers have been dropped
        map.retain(|_, tx| tx.receiver_count() > 0);

        if let Some(tx) = map.get(key) {
            return tx.clone();
        }
        let (tx, _) = broadcast::channel(1024);
        map.insert(key.to_string(), tx.clone());
        tx
    }

    /// Emit a payload to all subscribers of the given key.
    pub fn emit(&self, key: &str, payload: Value) {
        let tx = self.ensure_channel(key);
        let _ = tx.send(payload);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

pub fn new_event_bus() -> EventBus {
    EventBus::new()
}
