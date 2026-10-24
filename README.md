# brok-sdk

[![crates.io](https://img.shields.io/crates/v/brok-sdk.svg)](https://crates.io/crates/brok-sdk)
[![docs.rs](https://docs.rs/brok-sdk/badge.svg)](https://docs.rs/brok-sdk)

The contract between [Brokspace](https://github.com/brokspace/brok) and its
**shards**: the event protocol, the `shard.json` manifest and an async client
to join the local event bus.

```toml
[dependencies]
brok-sdk = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## A shard in 20 lines

```rust
use brok_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), ClientError> {
    // `brok run` provides the endpoint and token through the environment.
    let (client, mut inbox) = BrokClient::connect_from_env("todo-finder", env!("CARGO_PKG_VERSION")).await?;
    client.subscribe([TopicPattern::new("file:changed")?]).await?;

    while let Some(event) = inbox.recv().await {
        if event.is::<Shutdown>() {
            break; // system events reach every shard
        }
        if let Ok(change) = event.decode::<FileChanged>() {
            eprintln!("{} files changed", change.changed.len());
        }
    }
    Ok(())
}
```

Start from the [brok-shard-template](https://github.com/brokspace/brok-shard-template)
for a complete, tested project with CI and release automation.

## What is inside

| Module | Purpose |
|---|---|
| `client` | `BrokClient`: connect, subscribe, publish, `request`/`reply`, typed `call` |
| `payload` | Typed payloads for the well-known topics (`FileChanged`, `SearchRequest`, `AskRequest`, …) and the `Payload` trait for your own |
| `event` / `topic` | The event envelope and topic/pattern validation (`file:*`, `agent:ask.*`) |
| `manifest` | `ShardManifest`: parse, validate (all errors at once), capability report |
| `frame` / `codec` | Wire format: length-prefixed JSON frames, 16 MiB cap |
| `transport` | Unix domain sockets / Windows named pipes behind one API |
| `env` | Environment variables set by `brok run` |

## Request / reply

```rust
use std::time::Duration;
use brok_sdk::prelude::*;

async fn related_code(client: &BrokClient) -> Result<(), ClientError> {
    // Ask the index service (your manifest must allow publishing `index:search`).
    let request = SearchRequest { query: "retry policy".into(), limit: 5, focus: None };
    let results: SearchResults = client.call(&request, Duration::from_secs(10)).await?;
    for hit in results.hits {
        println!("{}:{} {}", hit.path, hit.start_line, hit.symbol.unwrap_or_default());
    }
    Ok(())
}
```

Replies are routed by the bus to the requester only.

## Your own events

```rust
use brok_sdk::Payload;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct TodoFound { pub path: String, pub line: u32, pub text: String }

impl Payload for TodoFound {
    const TOPIC: &'static str = "todo:found";
}
// client.emit(&TodoFound { … }).await?;
```

## Manifest schema

`shard.schema.json` describes `shard.json` for editors:

```json
{ "$schema": "https://raw.githubusercontent.com/brokspace/brok-sdk/main/shard.schema.json" }
```

The authoritative rules are in `ShardManifest::validate`; see the
[shard guide](https://github.com/brokspace/brok/blob/main/docs/shards.md).

## Compatibility

`PROTOCOL_VERSION` is exchanged in the handshake; the bus rejects clients
speaking another version. Additive changes keep the version; breaking changes
bump it together with the crate's major version.

## License

MIT or Apache-2.0, at your option.
