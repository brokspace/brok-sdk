//! The event envelope that travels on the bus.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::payload::Payload;
use crate::topic::Topic;

/// A single message on the Brokspace bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Unique id of this event.
    pub id: Uuid,
    /// Routing key.
    pub topic: Topic,
    /// Id of the event this one answers (request/reply and streaming replies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<Uuid>,
    /// Name of the publisher. Always overwritten by the bus with the
    /// authenticated identity, so it can be trusted by receivers.
    #[serde(default)]
    pub source: String,
    /// Milliseconds since the Unix epoch, set by the publisher.
    pub timestamp_ms: u64,
    /// Arbitrary JSON payload.
    #[serde(default)]
    pub payload: Value,
}

impl Event {
    /// Creates an event with a raw JSON payload.
    pub fn new(topic: Topic, payload: Value) -> Self {
        Self { id: Uuid::new_v4(), topic, correlation_id: None, source: String::new(), timestamp_ms: now_ms(), payload }
    }

    /// Creates an event from a typed [`Payload`].
    pub fn from_payload<P: Payload>(payload: &P) -> Result<Self, serde_json::Error> {
        let topic = Topic::new(P::TOPIC).expect("Payload::TOPIC must be a valid topic");
        Ok(Self::new(topic, serde_json::to_value(payload)?))
    }

    /// Marks this event as a reply to `request`.
    #[must_use]
    pub fn in_reply_to(mut self, request: &Event) -> Self {
        self.correlation_id = Some(request.id);
        self
    }

    /// Whether the event carries the given payload type.
    pub fn is<P: Payload>(&self) -> bool {
        self.topic.as_str() == P::TOPIC
    }

    /// Decodes the payload as `P`, checking that the topic matches.
    pub fn decode<P: Payload>(&self) -> Result<P, serde_json::Error> {
        if !self.is::<P>() {
            return Err(serde::de::Error::custom(format!("expected topic `{}`, got `{}`", P::TOPIC, self.topic)));
        }
        P::deserialize(&self.payload)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::{AskChunk, FileChanged};

    #[test]
    fn typed_roundtrip() {
        let payload = FileChanged { changed: vec!["src/main.rs".into()], deleted: vec![] };
        let event = Event::from_payload(&payload).unwrap();
        assert_eq!(event.topic.as_str(), "file:changed");
        assert_eq!(event.decode::<FileChanged>().unwrap(), payload);
        assert!(event.decode::<AskChunk>().is_err());
    }

    #[test]
    fn replies_carry_correlation() {
        let req = Event::new(Topic::new("agent:ask").unwrap(), Value::Null);
        let reply = Event::from_payload(&AskChunk { text: "hi".into() }).unwrap().in_reply_to(&req);
        assert_eq!(reply.correlation_id, Some(req.id));
    }
}
