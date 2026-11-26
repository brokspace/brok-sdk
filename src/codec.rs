//! Length-delimited JSON framing.
//!
//! Each frame is a 4-byte big-endian length followed by a UTF-8 JSON
//! document. Frames larger than [`MAX_FRAME_BYTES`] are rejected before any
//! allocation happens, which protects both sides from memory exhaustion.

use std::io;
use std::marker::PhantomData;

use bytes::{Bytes, BytesMut};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

use crate::frame::{ClientFrame, ServerFrame};

/// Upper bound for a single frame (16 MiB).
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Errors raised while framing.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// I/O failure or oversized frame.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The frame is not valid JSON for the expected type.
    #[error("invalid frame: {0}")]
    Json(#[from] serde_json::Error),
}

/// A codec that writes `Out` frames and reads `In` frames.
#[derive(Debug)]
pub struct JsonCodec<Out, In> {
    inner: LengthDelimitedCodec,
    _marker: PhantomData<fn(Out) -> In>,
}

impl<Out, In> JsonCodec<Out, In> {
    /// Creates a codec with the default frame limit.
    pub fn new() -> Self {
        Self::with_max_frame(MAX_FRAME_BYTES)
    }

    /// Creates a codec with a custom frame limit.
    pub fn with_max_frame(max: usize) -> Self {
        let inner = LengthDelimitedCodec::builder().max_frame_length(max).new_codec();
        Self { inner, _marker: PhantomData }
    }
}

impl<Out, In> Default for JsonCodec<Out, In> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Out, In: DeserializeOwned> Decoder for JsonCodec<Out, In> {
    type Item = In;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<In>, CodecError> {
        match self.inner.decode(src)? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }
}

impl<Out: Serialize, In> Encoder<Out> for JsonCodec<Out, In> {
    type Error = CodecError;

    fn encode(&mut self, item: Out, dst: &mut BytesMut) -> Result<(), CodecError> {
        let bytes = serde_json::to_vec(&item)?;
        self.inner.encode(Bytes::from(bytes), dst)?;
        Ok(())
    }
}

/// A codec that also reports the size of each frame it reads.
#[derive(Debug, Default)]
pub(crate) struct Metered<C> {
    inner: C,
    /// Length of the frame being read; the length codec consumes the prefix
    /// as soon as it arrives, often before the rest of the frame.
    size: Option<usize>,
}

impl<C> Metered<C> {
    pub(crate) fn new(inner: C) -> Self {
        Self { inner, size: None }
    }
}

impl<C: Decoder> Decoder for Metered<C> {
    /// The frame's payload length in bytes, and the frame.
    type Item = (usize, C::Item);
    type Error = C::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, C::Error> {
        if self.size.is_none() {
            self.size = src.first_chunk::<4>().map(|prefix| u32::from_be_bytes(*prefix) as usize);
        }
        Ok(self.inner.decode(src)?.map(|frame| (self.size.take().unwrap_or_default(), frame)))
    }
}

impl<I, C: Encoder<I>> Encoder<I> for Metered<C> {
    type Error = C::Error;

    fn encode(&mut self, item: I, dst: &mut BytesMut) -> Result<(), C::Error> {
        self.inner.encode(item, dst)
    }
}

/// Codec used by clients: writes [`ClientFrame`], reads [`ServerFrame`].
pub type ClientCodec = JsonCodec<ClientFrame, ServerFrame>;
/// Codec used by the bus: writes [`ServerFrame`], reads [`ClientFrame`].
pub type ServerCodec = JsonCodec<ServerFrame, ClientFrame>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::ErrorCode;
    use crate::topic::TopicPattern;

    #[test]
    fn roundtrip_between_client_and_server() {
        let mut client = ClientCodec::new();
        let mut server = ServerCodec::new();
        let frame = ClientFrame::Subscribe { patterns: vec![TopicPattern::any()] };

        let mut buf = BytesMut::new();
        client.encode(frame.clone(), &mut buf).unwrap();
        assert_eq!(server.decode(&mut buf).unwrap(), Some(frame));
        assert!(buf.is_empty());
    }

    #[test]
    fn partial_frames_wait_for_more_bytes() {
        let mut client = ClientCodec::new();
        let mut server = ServerCodec::new();
        let mut full = BytesMut::new();
        client.encode(ClientFrame::Subscribe { patterns: vec![] }, &mut full).unwrap();

        let mut partial = full.split_to(full.len() - 1);
        assert_eq!(server.decode(&mut partial).unwrap(), None);
        partial.unsplit(full);
        assert!(server.decode(&mut partial).unwrap().is_some());
    }

    #[test]
    fn metered_reports_frame_sizes_however_the_bytes_arrive() {
        let mut server = ServerCodec::new();
        let mut wire = BytesMut::new();
        let mut sizes = Vec::new();
        for identity in ["a", "a-much-longer-message"] {
            let before = wire.len();
            let frame =
                ServerFrame::Error { code: ErrorCode::Forbidden, message: identity.into(), correlation_id: None };
            server.encode(frame, &mut wire).unwrap();
            sizes.push(wire.len() - before - 4);
        }
        for chunk in [1, 3, 7, wire.len()] {
            let mut client = Metered::new(ClientCodec::new());
            let (mut buf, mut seen) = (BytesMut::new(), Vec::new());
            for piece in wire.chunks(chunk) {
                buf.extend_from_slice(piece);
                while let Some((size, _)) = client.decode(&mut buf).unwrap() {
                    seen.push(size);
                }
            }
            assert_eq!(seen, sizes, "in pieces of {chunk}");
        }
    }

    #[test]
    fn rejects_oversized_frames() {
        let mut server = ServerCodec::with_max_frame(8);
        let mut buf = BytesMut::from(&[0u8, 0, 0, 64][..]);
        buf.extend_from_slice(&[b' '; 64]);
        assert!(matches!(server.decode(&mut buf), Err(CodecError::Io(_))));
    }
}
