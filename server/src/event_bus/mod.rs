use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub type EventBus = Arc<Mutex<HashMap<String, broadcast::Sender<Value>>>>;

pub fn new_event_bus() -> EventBus {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn ensure_channel(bus: &EventBus, key: &str) -> broadcast::Sender<Value> {
    let mut map = bus.lock().expect("event bus lock");
    if let Some(tx) = map.get(key) {
        return tx.clone();
    }
    let (tx, _) = broadcast::channel(1024);
    map.insert(key.to_string(), tx.clone());
    tx
}

pub fn emit(bus: &EventBus, key: &str, payload: Value) {
    let tx = ensure_channel(bus, key);
    let _ = tx.send(payload);
}
