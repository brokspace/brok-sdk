//! Error types exposed by the SDK.

use crate::frame::ErrorCode;

/// A topic or topic pattern failed validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid topic `{value}`: {reason}")]
pub struct TopicError {
    /// The rejected input.
    pub value: String,
    /// Human readable reason.
    pub reason: &'static str,
}

/// A `shard.json` manifest could not be loaded or is invalid.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The manifest file could not be read.
    #[error("cannot read manifest {path}: {source}")]
    Io {
        /// Path that was read.
        path: std::path::PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The manifest is not valid JSON or does not match the schema.
    #[error("malformed manifest: {0}")]
    Parse(#[from] serde_json::Error),
    /// The manifest parsed but violates one or more rules.
    #[error("invalid manifest:\n  - {}", .0.join("\n  - "))]
    Invalid(Vec<String>),
}

/// Errors produced by [`crate::BrokClient`].
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Transport level I/O failure.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// A frame could not be encoded or decoded.
    #[error("wire format error: {0}")]
    Codec(#[from] crate::codec::CodecError),
    /// A payload could not be (de)serialized.
    #[error("payload error: {0}")]
    Payload(#[from] serde_json::Error),
    /// The bus answered the handshake with something unexpected.
    #[error("handshake failed: {0}")]
    Handshake(String),
    /// The bus rejected a request.
    #[error("bus rejected request ({code:?}): {message}")]
    Rejected {
        /// Machine readable reason.
        code: ErrorCode,
        /// Human readable message.
        message: String,
    },
    /// The connection to the bus is closed.
    #[error("connection to the bus is closed")]
    Closed,
    /// No reply arrived in time.
    #[error("timed out waiting for a reply")]
    Timeout,
    /// A required environment variable is missing (shard not launched by `brok run`).
    #[error("environment variable {0} is not set; is this shard running under `brok run`?")]
    MissingEnv(&'static str),
    /// Invalid topic.
    #[error(transparent)]
    Topic(#[from] TopicError),
}
