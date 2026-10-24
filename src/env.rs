//! Environment variables set by `brok run` for every shard process.

/// IPC endpoint of the bus (socket path or pipe name).
pub const ENDPOINT: &str = "BROK_ENDPOINT";
/// Per-process session token. Never log it.
pub const TOKEN: &str = "BROK_TOKEN";
/// Name of the shard, as declared in its manifest.
pub const SHARD_NAME: &str = "BROK_SHARD_NAME";
/// Absolute path of the workspace root (also the process working directory).
pub const WORKSPACE: &str = "BROK_WORKSPACE";
/// Workspace relative paths the shard declared as readable, `;`-separated.
pub const FS_READ: &str = "BROK_FS_READ";
/// Workspace relative paths the shard declared as writable, `;`-separated.
pub const FS_WRITE: &str = "BROK_FS_WRITE";
/// `1` when the shard declared network access, `0` otherwise.
pub const NETWORK: &str = "BROK_NETWORK";

/// Prefix reserved for runtime variables; manifests cannot request it.
pub const RESERVED_PREFIX: &str = "BROK_";
