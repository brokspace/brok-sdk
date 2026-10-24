//! # brok-sdk
//!
//! The shared contract between the Brokspace runtime (`brok`) and its shards.
//!
//! A *shard* is a standalone process that joins the local Brokspace event bus,
//! subscribes to topics it cares about and publishes new events. This crate
//! gives shard authors everything they need:
//!
//! - [`event`] / [`topic`]: the event envelope and topic addressing rules.
//! - [`payload`]: typed payloads for the well-known Brokspace topics.
//! - [`manifest`]: the `shard.json` schema and its validation rules.
//! - [`client`]: an async client that connects to the bus, with pub/sub and
//!   request/reply helpers.
//! - [`transport`] / [`codec`]: the cross-platform IPC transport (Unix domain
//!   sockets or Windows named pipes) and the length-delimited JSON wire format.
//!
//! ```no_run
//! use brok_sdk::prelude::*;
//!
//! # async fn run() -> Result<(), brok_sdk::ClientError> {
//! let (client, mut inbox) = BrokClient::connect_from_env("my-shard", "0.1.0").await?;
//! client.subscribe([TopicPattern::new("file:changed")?]).await?;
//!
//! while let Some(event) = inbox.recv().await {
//!     if let Ok(change) = event.decode::<FileChanged>() {
//!         println!("{} files changed", change.changed.len());
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod codec;
pub mod env;
pub mod error;
pub mod event;
pub mod frame;
pub mod manifest;
pub mod payload;
pub mod topic;
pub mod transport;

pub use client::{BrokClient, ConnectOptions, Inbox, Replies};
pub use error::{ClientError, ManifestError, TopicError};
pub use event::Event;
pub use frame::{ClientFrame, ErrorCode, ServerFrame, ShardContext, Welcome};
pub use manifest::ShardManifest;
pub use payload::Payload;
pub use topic::{Topic, TopicPattern};
pub use transport::Endpoint;

/// Version of the wire protocol spoken between shards and the bus.
///
/// Bumped on any breaking change to [`frame`]. The bus rejects clients whose
/// protocol version differs from its own.
pub const PROTOCOL_VERSION: u32 = 1;

/// Convenient glob import for shard authors.
pub mod prelude {
    pub use crate::client::{BrokClient, ConnectOptions, Inbox, Replies};
    pub use crate::error::ClientError;
    pub use crate::event::Event;
    pub use crate::payload::*;
    pub use crate::topic::{Topic, TopicPattern};
}
