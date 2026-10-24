//! End-to-end test of the client against a minimal in-test bus, over the
//! real platform transport (Unix socket or Windows named pipe).

use std::time::Duration;

use brok_sdk::codec::ServerCodec;
use brok_sdk::frame::{ClientFrame, ErrorCode, ServerFrame, ShardContext, Welcome};
use brok_sdk::prelude::*;
use brok_sdk::transport::{Endpoint, Listener};
use futures_util::{SinkExt, StreamExt};
use tokio_util::codec::Framed;

fn unique_endpoint(dir: &std::path::Path) -> Endpoint {
    if cfg!(windows) {
        Endpoint::new(format!(r"\\.\pipe\brok-test-{}", uuid::Uuid::new_v4().simple()))
    } else {
        Endpoint::new(dir.join("bus.sock").to_string_lossy().into_owned())
    }
}

/// A fake bus that echoes `agent:ask` requests as two chunks and rejects
/// everything published on `forbidden:*`.
async fn fake_bus(mut listener: Listener) {
    let stream = listener.accept().await.unwrap();
    let mut framed = Framed::new(stream, ServerCodec::new());

    let Some(Ok(ClientFrame::Hello(hello))) = framed.next().await else { panic!("expected hello") };
    if hello.token != "s3cret" {
        framed
            .send(ServerFrame::Error {
                code: ErrorCode::Unauthorized,
                message: "bad token".into(),
                correlation_id: None,
            })
            .await
            .unwrap();
        return;
    }
    framed
        .send(ServerFrame::Welcome(Welcome {
            protocol: brok_sdk::PROTOCOL_VERSION,
            session_id: uuid::Uuid::new_v4(),
            identity: hello.name,
            context: ShardContext::default(),
        }))
        .await
        .unwrap();

    while let Some(Ok(frame)) = framed.next().await {
        match frame {
            ClientFrame::Subscribe { .. } => {
                let event =
                    Event::from_payload(&FileChanged { changed: vec!["a.rs".into()], deleted: vec![] }).unwrap();
                framed.send(ServerFrame::Event { event }).await.unwrap();
            }
            ClientFrame::Publish { event } if event.topic.namespace() == "forbidden" => {
                framed
                    .send(ServerFrame::Error {
                        code: ErrorCode::Forbidden,
                        message: "nope".into(),
                        correlation_id: Some(event.id),
                    })
                    .await
                    .unwrap();
            }
            ClientFrame::Publish { event } => {
                let ask: AskRequest = event.decode().unwrap();
                for text in [ask.prompt.as_str(), "!"] {
                    let reply = Event::from_payload(&AskChunk { text: text.into() }).unwrap().in_reply_to(&event);
                    framed.send(ServerFrame::Event { event: reply }).await.unwrap();
                }
            }
            ClientFrame::Hello(_) => panic!("duplicate hello"),
        }
    }
}

#[tokio::test]
async fn pubsub_and_request_reply() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = unique_endpoint(dir.path());
    let bus = tokio::spawn(fake_bus(Listener::bind(&endpoint).unwrap()));

    let (client, mut inbox) =
        BrokClient::connect(&endpoint, ConnectOptions::new("test", "0.0.0", "s3cret")).await.unwrap();
    assert_eq!(client.identity(), "test");

    client.subscribe([TopicPattern::new("file:*").unwrap()]).await.unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), inbox.recv()).await.unwrap().unwrap();
    assert_eq!(event.decode::<FileChanged>().unwrap().changed, vec!["a.rs"]);

    let ask = AskRequest { prompt: "hello".into(), provider: None, context_chunks: None, skill: None, history: vec![] };
    let mut replies = client.request(Event::from_payload(&ask).unwrap()).await.unwrap();
    let mut answer = String::new();
    for _ in 0..2 {
        let chunk = replies.next_timeout(Duration::from_secs(5)).await.unwrap();
        answer.push_str(&chunk.decode::<AskChunk>().unwrap().text);
    }
    assert_eq!(answer, "hello!");

    let forbidden = Event::new(Topic::new("forbidden:thing").unwrap(), serde_json::Value::Null);
    let mut replies = client.request(forbidden).await.unwrap();
    let err = replies.next_timeout(Duration::from_secs(5)).await.unwrap_err();
    assert!(matches!(err, ClientError::Rejected { code: ErrorCode::Forbidden, .. }));

    drop((client, inbox, replies));
    tokio::time::timeout(Duration::from_secs(5), bus).await.unwrap().unwrap();
}

#[tokio::test]
async fn bad_token_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = unique_endpoint(dir.path());
    let bus = tokio::spawn(fake_bus(Listener::bind(&endpoint).unwrap()));

    let err = BrokClient::connect(&endpoint, ConnectOptions::new("test", "0.0.0", "wrong")).await.unwrap_err();
    assert!(matches!(err, ClientError::Rejected { code: ErrorCode::Unauthorized, .. }));
    bus.await.unwrap();
}
