//! Global event bus that aggregates events from all directory-scoped buses.

use std::collections::HashSet;

use tokio::sync::broadcast;

use super::EventBus;
use super::events::{EventTopic, RuntimeEvent};
use super::subscribers::TopicSubscriber;

/// Aggregates events from multiple directory-scoped [`EventBus`] instances.
///
/// Subscribers to the `GlobalEventBus` receive events from ALL directories.
/// Useful for cross-cutting concerns like cost tracking and system metrics.
pub struct GlobalEventBus {
    inner: EventBus,
}

impl GlobalEventBus {
    /// Create a new global event bus.
    pub fn new() -> Self {
        Self {
            inner: EventBus::new(),
        }
    }

    /// Subscribe to events from all directories.
    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.inner.subscribe()
    }

    /// Subscribe with topic filtering across all directories.
    pub fn subscribe_topics(&self, topics: HashSet<EventTopic>) -> TopicSubscriber {
        self.inner.subscribe_topics(topics)
    }

    /// Forward an event from a directory-scoped bus to the global bus.
    pub fn forward(&self, event: RuntimeEvent) {
        self.inner.publish(event);
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.inner.subscriber_count()
    }
}

impl Default for GlobalEventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GlobalEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalEventBus")
            .field("subscribers", &self.subscriber_count())
            .finish()
    }
}
