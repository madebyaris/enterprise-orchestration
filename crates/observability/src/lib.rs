use domain::EventEnvelope;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<EventEnvelope>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn publish(&self, event: EventEnvelope) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use domain::{EventEnvelope, EventScope};

    use super::EventBus;

    #[tokio::test]
    async fn publishes_events_to_subscribers() {
        let bus = EventBus::new(8);
        let mut receiver = bus.subscribe();

        let event = EventEnvelope::new(
            EventScope::System,
            "system.test",
            "Test event",
            serde_json::json!({"ok": true}),
        );

        bus.publish(event.clone());

        let received = receiver.recv().await.expect("event");
        assert_eq!(received.event_type, event.event_type);
        assert_eq!(received.summary, "Test event");
    }
}
