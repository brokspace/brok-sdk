//! Async client for the Brokspace bus.
//!
//! [`BrokClient::connect`] performs the handshake and returns two halves:
//!
//! - a cloneable [`BrokClient`] used to subscribe, publish and make requests;
//! - an [`Inbox`] yielding events that matched the client's subscriptions.
//!
//! Replies to events published through [`BrokClient::request`] never reach
//! the inbox: they are routed to the [`Replies`] handle of that request.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_util::codec::Framed;
use uuid::Uuid;

use crate::codec::ClientCodec;
use crate::env;
use crate::error::ClientError;
use crate::event::Event;
use crate::frame::{ClientFrame, Hello, ServerFrame, ShardContext, Welcome};
use crate::payload::Payload;
use crate::topic::TopicPattern;
use crate::transport::{self, Endpoint};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

type ReplySender = mpsc::Sender<Result<Event, ClientError>>;
type PendingMap = Mutex<HashMap<Uuid, ReplySender>>;

/// Options for [`BrokClient::connect`].
#[derive(Clone)]
pub struct ConnectOptions {
    /// Client name, informational.
    pub name: String,
    /// Client version, informational.
    pub version: String,
    /// Session token.
    pub token: String,
    /// Buffered events before backpressure applies (default 256).
    pub inbox_capacity: usize,
}

impl ConnectOptions {
    /// Options with default capacities.
    pub fn new(name: impl Into<String>, version: impl Into<String>, token: impl Into<String>) -> Self {
        Self { name: name.into(), version: version.into(), token: token.into(), inbox_capacity: 256 }
    }
}

impl std::fmt::Debug for ConnectOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectOptions")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("token", &"<redacted>")
            .field("inbox_capacity", &self.inbox_capacity)
            .finish()
    }
}

/// Handle to a bus session. Cheap to clone; the connection closes when the
/// last clone is dropped.
#[derive(Clone, Debug)]
pub struct BrokClient {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    outgoing: mpsc::Sender<ClientFrame>,
    pending: Arc<PendingMap>,
    welcome: Welcome,
}

/// Events delivered because they matched a subscription.
#[derive(Debug)]
pub struct Inbox {
    rx: mpsc::Receiver<Event>,
}

impl Inbox {
    /// Waits for the next event. Returns `None` once the connection closed.
    pub async fn recv(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

/// Stream of replies to one request. Dropping it unregisters the request.
#[derive(Debug)]
pub struct Replies {
    id: Uuid,
    rx: mpsc::Receiver<Result<Event, ClientError>>,
    pending: Weak<PendingMap>,
}

impl Replies {
    /// Id of the request event.
    pub fn request_id(&self) -> Uuid {
        self.id
    }

    /// Next reply, `None` when the connection closed.
    pub async fn next(&mut self) -> Option<Result<Event, ClientError>> {
        self.rx.recv().await
    }

    /// Next reply, failing with [`ClientError::Timeout`] after `timeout`.
    pub async fn next_timeout(&mut self, timeout: Duration) -> Result<Event, ClientError> {
        match tokio::time::timeout(timeout, self.rx.recv()).await {
            Ok(Some(reply)) => reply,
            Ok(None) => Err(ClientError::Closed),
            Err(_) => Err(ClientError::Timeout),
        }
    }
}

impl Drop for Replies {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.upgrade() {
            pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id);
        }
    }
}

impl BrokClient {
    /// Connects using the variables injected by `brok run`
    /// ([`env::ENDPOINT`] and [`env::TOKEN`]).
    pub async fn connect_from_env(name: &str, version: &str) -> Result<(Self, Inbox), ClientError> {
        let endpoint = std::env::var(env::ENDPOINT).map_err(|_| ClientError::MissingEnv(env::ENDPOINT))?;
        let token = std::env::var(env::TOKEN).map_err(|_| ClientError::MissingEnv(env::TOKEN))?;
        Self::connect(&Endpoint::new(endpoint), ConnectOptions::new(name, version, token)).await
    }

    /// Connects to `endpoint` and performs the handshake.
    pub async fn connect(endpoint: &Endpoint, options: ConnectOptions) -> Result<(Self, Inbox), ClientError> {
        let stream = transport::connect(endpoint).await?;
        let mut framed = Framed::new(stream, ClientCodec::new());

        framed
            .send(ClientFrame::Hello(Hello {
                protocol: crate::PROTOCOL_VERSION,
                name: options.name.clone(),
                version: options.version.clone(),
                token: options.token.clone(),
            }))
            .await?;

        let welcome = match tokio::time::timeout(HANDSHAKE_TIMEOUT, framed.next()).await {
            Err(_) => return Err(ClientError::Timeout),
            Ok(None) => return Err(ClientError::Closed),
            Ok(Some(Err(e))) => return Err(e.into()),
            Ok(Some(Ok(ServerFrame::Welcome(welcome)))) => welcome,
            Ok(Some(Ok(ServerFrame::Error { code, message, .. }))) => {
                return Err(ClientError::Rejected { code, message });
            }
            Ok(Some(Ok(other))) => return Err(ClientError::Handshake(format!("unexpected frame {other:?}"))),
        };

        let (mut sink, mut stream) = framed.split();
        let (out_tx, mut out_rx) = mpsc::channel::<ClientFrame>(64);
        let (inbox_tx, inbox_rx) = mpsc::channel::<Event>(options.inbox_capacity.max(1));
        let pending: Arc<PendingMap> = Arc::default();
        // Named pipes cannot be half-closed, so the writer tells the reader to
        // drop its half as well; only then is the handle actually released.
        let (writer_done_tx, mut writer_done_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if let Err(e) = sink.send(frame).await {
                    tracing::debug!(error = %e, "bus writer stopped");
                    break;
                }
            }
            let _ = sink.close().await;
            drop(writer_done_tx);
        });

        let reader_pending = Arc::clone(&pending);
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    frame = stream.next() => match frame {
                        Some(frame) => frame,
                        None => break,
                    },
                    _ = &mut writer_done_rx => break,
                };
                match frame {
                    Ok(ServerFrame::Event { event }) => {
                        if let Some(tx) = reply_slot(&reader_pending, event.correlation_id) {
                            let _ = tx.send(Ok(event)).await;
                        } else if inbox_tx.send(event).await.is_err() {
                            tracing::trace!("inbox dropped; discarding event");
                        }
                    }
                    Ok(ServerFrame::Error { code, message, correlation_id }) => {
                        if let Some(tx) = reply_slot(&reader_pending, correlation_id) {
                            let _ = tx.send(Err(ClientError::Rejected { code, message })).await;
                        } else {
                            tracing::warn!(?code, %message, "bus reported an error");
                        }
                    }
                    Ok(ServerFrame::Welcome(_)) => tracing::warn!("ignoring unexpected welcome frame"),
                    Err(e) => {
                        tracing::warn!(error = %e, "bus connection failed");
                        break;
                    }
                }
            }
            // Wake up everyone waiting for replies.
            reader_pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
        });

        let client = Self { inner: Arc::new(Inner { outgoing: out_tx, pending, welcome }) };
        Ok((client, Inbox { rx: inbox_rx }))
    }

    /// Context handed over by the runtime (workspace root, settings, llm routing).
    pub fn context(&self) -> &ShardContext {
        &self.inner.welcome.context
    }

    /// Authenticated identity of this session.
    pub fn identity(&self) -> &str {
        &self.inner.welcome.identity
    }

    /// Adds subscriptions.
    pub async fn subscribe(&self, patterns: impl IntoIterator<Item = TopicPattern>) -> Result<(), ClientError> {
        self.send(ClientFrame::Subscribe { patterns: patterns.into_iter().collect() }).await
    }

    /// Publishes a raw event.
    pub async fn publish(&self, event: Event) -> Result<(), ClientError> {
        self.send(ClientFrame::Publish { event }).await
    }

    /// Publishes a typed payload and returns the event id.
    pub async fn emit<P: Payload>(&self, payload: &P) -> Result<Uuid, ClientError> {
        let event = Event::from_payload(payload)?;
        let id = event.id;
        self.publish(event).await?;
        Ok(id)
    }

    /// Publishes a typed reply to `request`.
    pub async fn reply<P: Payload>(&self, request: &Event, payload: &P) -> Result<(), ClientError> {
        self.publish(Event::from_payload(payload)?.in_reply_to(request)).await
    }

    /// Publishes `event` and returns a handle receiving every reply to it.
    pub async fn request(&self, event: Event) -> Result<Replies, ClientError> {
        let (tx, rx) = mpsc::channel(64);
        let id = event.id;
        self.inner.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        let replies = Replies { id, rx, pending: Arc::downgrade(&self.inner.pending) };
        self.publish(event).await?;
        Ok(replies)
    }

    /// Typed request/response: publishes `request` and waits for the first
    /// reply carrying a `Res` payload.
    pub async fn call<Req: Payload, Res: Payload>(&self, request: &Req, timeout: Duration) -> Result<Res, ClientError> {
        let mut replies = self.request(Event::from_payload(request)?).await?;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let reply = replies.next_timeout(remaining).await?;
            if reply.is::<Res>() {
                return Ok(reply.decode()?);
            }
        }
    }

    async fn send(&self, frame: ClientFrame) -> Result<(), ClientError> {
        self.inner.outgoing.send(frame).await.map_err(|_| ClientError::Closed)
    }
}

fn reply_slot(pending: &PendingMap, correlation_id: Option<Uuid>) -> Option<ReplySender> {
    let id = correlation_id?;
    pending.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned()
}
