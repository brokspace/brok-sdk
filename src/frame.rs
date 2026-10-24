//! Wire frames exchanged between clients (shards, CLI) and the bus.
//!
//! The session is:
//!
//! 1. client sends [`ClientFrame::Hello`] with its token,
//! 2. bus answers [`ServerFrame::Welcome`] (or [`ServerFrame::Error`] and closes),
//! 3. client sends any number of `Subscribe` / `Publish` frames,
//! 4. bus pushes matching [`ServerFrame::Event`]s and reports rejected
//!    requests with [`ServerFrame::Error`].

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::event::Event;
use crate::topic::TopicPattern;

/// Frames sent by a client to the bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientFrame {
    /// First frame of every session.
    Hello(Hello),
    /// Adds subscriptions. Patterns must be allowed by the client's grants.
    Subscribe {
        /// Patterns to subscribe to.
        patterns: Vec<TopicPattern>,
    },
    /// Publishes an event.
    Publish {
        /// The event to route.
        event: Event,
    },
}

/// Handshake payload.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// Protocol version spoken by the client ([`crate::PROTOCOL_VERSION`]).
    pub protocol: u32,
    /// Client name (shard name, `brok-cli`, ...). Informational only; the
    /// authoritative identity is derived from the token.
    pub name: String,
    /// Client version.
    pub version: String,
    /// Session token handed out by `brok run`.
    pub token: String,
}

impl fmt::Debug for Hello {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hello")
            .field("protocol", &self.protocol)
            .field("name", &self.name)
            .field("version", &self.version)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Frames sent by the bus to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    /// Successful handshake.
    Welcome(Welcome),
    /// An event matching one of the client's subscriptions, or a reply to
    /// an event the client published.
    Event {
        /// The routed event.
        event: Event,
    },
    /// A request was rejected.
    Error {
        /// Machine readable reason.
        code: ErrorCode,
        /// Human readable message.
        message: String,
        /// Id of the offending event, when the error relates to a publish.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        correlation_id: Option<Uuid>,
    },
}

/// Handshake answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    /// Protocol version spoken by the bus.
    pub protocol: u32,
    /// Unique id of this session.
    pub session_id: Uuid,
    /// Authenticated identity (shard name or `operator`).
    pub identity: String,
    /// Runtime context for the client.
    pub context: ShardContext,
}

/// Context handed to a client on connect.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShardContext {
    /// Absolute path of the workspace root.
    pub workspace_root: PathBuf,
    /// The shard's own section of `brok.yml` (`shards.<name>.settings`).
    #[serde(default)]
    pub settings: Value,
    /// The global `llm` section of `brok.yml` (provider routing). API keys
    /// are never included: providers reference environment variables.
    #[serde(default)]
    pub llm: Value,
}

/// Reason a request was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Missing or invalid token.
    Unauthorized,
    /// The identity lacks the permission for this topic.
    Forbidden,
    /// Malformed frame or frame not valid in the current state.
    InvalidFrame,
    /// Client and bus protocol versions differ.
    ProtocolMismatch,
    /// The bus failed internally.
    Internal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_tagged() {
        let frame = ClientFrame::Subscribe { patterns: vec![TopicPattern::new("file:*").unwrap()] };
        let json = serde_json::to_string(&frame).unwrap();
        assert_eq!(json, r#"{"type":"subscribe","patterns":["file:*"]}"#);
        assert_eq!(serde_json::from_str::<ClientFrame>(&json).unwrap(), frame);
    }

    #[test]
    fn hello_debug_redacts_token() {
        let hello = Hello { protocol: 1, name: "x".into(), version: "0".into(), token: "secret".into() };
        assert!(!format!("{hello:?}").contains("secret"));
    }
}
