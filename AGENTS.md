# AGENTS.md

Operating guide for coding agents and contributors working in `brokspace/brok-sdk`: the contract between the Brokspace runtime and its shards (event protocol, `shard.json` manifest, IPC transport and the async Rust client).

This crate is a **public contract** used by third-party shards. Every change is weighed against the shards already in the wild. Read the project scope in [brok's SCOPE.md](https://github.com/brokspace/brok/blob/main/SCOPE.md) before proposing new API.

## Layout

```text
src/
  client.rs     BrokClient: connect, subscribe, publish, request/reply, typed call
  codec.rs      length-prefixed JSON frames (16 MiB cap)
  env.rs        environment variables set by `brok run`
  error.rs      public error types
  event.rs      the event envelope
  frame.rs      wire frames (Hello, Welcome, Subscribe, Publish, Event, Error)
  manifest.rs   ShardManifest: parse, validate (all errors at once), capability report
  payload.rs    typed payloads for well-known topics, the Payload trait
  topic.rs      topics and topic patterns
  transport.rs  Unix domain sockets / Windows named pipes
tests/          end-to-end client tests over the real transport
shard.schema.json  JSON Schema for shard.json (keep in sync with manifest.rs)
```

## Build and test

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo doc --no-deps    # RUSTDOCFLAGS=-D warnings in CI
```

Pure Rust, no native toolchain needed. The other Brokspace repositories build against this checkout through the polyrepo bootstrap (`brok/scripts/bootstrap.sh` writes `../.cargo/config.toml` with a `[patch.crates-io]` entry), so run their tests too when you change public API.

## Conventions

- `unsafe` is forbidden. `missing_docs` is on: every public item has a doc comment.
- Compatibility rules:
  - additive changes (new optional fields with `#[serde(default)]`, new topics, new payloads) are fine in minor releases;
  - anything that changes the wire format or rejects previously valid manifests bumps `PROTOCOL_VERSION` and the crate's major version, and needs an ADR in [brok/docs/adr](https://github.com/brokspace/brok/tree/main/docs/adr) first.
- Manifest changes update three places together: `manifest.rs` (types and `validate`), `shard.schema.json`, and the shard guide in [brok/docs/shards.md](https://github.com/brokspace/brok/blob/main/docs/shards.md).
- Secrets never reach `Debug` output (see `Hello` and `ConnectOptions`).
- Tests run over the real platform transport (Unix sockets or named pipes); keep them cross-platform.
- Commit messages and PR titles follow Conventional Commits: `feat(client): ...`, `fix(manifest): ...`.
- No em-dashes in prose.

## Agent skills configuration

- **Issue tracker**: GitHub Issues on [brokspace/brok-sdk](https://github.com/brokspace/brok-sdk/issues) via `gh`. See [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md).
- **Triage labels**: [docs/agents/triage-labels.md](docs/agents/triage-labels.md). Issues are judged against [brok's SCOPE.md](https://github.com/brokspace/brok/blob/main/SCOPE.md).
- **Domain docs**: the shared glossary is [brok's GLOSSARY.md](https://github.com/brokspace/brok/blob/main/GLOSSARY.md); decisions live in [brok/docs/adr](https://github.com/brokspace/brok/tree/main/docs/adr).
