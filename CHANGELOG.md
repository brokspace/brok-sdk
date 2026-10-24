# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the crate
adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-24

### Added

- Wire protocol v1: `Hello` / `Welcome` handshake, `Subscribe`, `Publish`,
  `Event` and `Error` frames as length-prefixed JSON (16 MiB cap).
- `Event` envelope with correlation ids, typed `Payload` trait and payloads
  for the well-known topics.
- Topic and topic pattern validation with coverage checks.
- `ShardManifest` (`shard.json`) parsing, exhaustive validation and
  capability reports, plus `shard.schema.json` for editors.
- Skill shards: `skills` directories in `shard.json` (`entrypoint` optional
  for skill-only shards) and `SkillInvocation` on `AskRequest`.
- Cross-platform transport (Unix domain sockets, Windows named pipes).
- `BrokClient` with pub/sub, request/reply streams and typed calls.
