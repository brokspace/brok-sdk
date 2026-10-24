//! Cross-platform local IPC transport.
//!
//! - **Linux / macOS**: Unix domain sockets, created with `0600` permissions.
//! - **Windows**: named pipes that reject remote clients.
//!
//! Both sides only ever see a [`BoxedStream`], so the rest of the stack is
//! platform independent.

use std::fmt;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

/// A bidirectional byte stream usable by the codec layer.
pub trait IpcStream: AsyncRead + AsyncWrite + Send + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> IpcStream for T {}

/// Type-erased IPC stream.
pub type BoxedStream = Box<dyn IpcStream>;

/// Address of a bus: a socket path on Unix, a pipe name on Windows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Endpoint(String);

impl Endpoint {
    /// Wraps a raw socket path or pipe name.
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// The raw address.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The canonical endpoint for the workspace rooted at `root`.
    ///
    /// The same workspace always maps to the same endpoint, so the CLI can
    /// find a running `brok run` without any extra configuration.
    pub fn for_workspace(root: &Path) -> Self {
        let id = workspace_id(root);
        #[cfg(windows)]
        {
            Self(format!(r"\\.\pipe\brok-{id:016x}"))
        }
        #[cfg(unix)]
        {
            // sun_path is limited to ~104 bytes on macOS and 108 on Linux.
            let preferred = root.join(".brokspace").join("run").join("brok.sock");
            if preferred.as_os_str().len() < 100 {
                Self(preferred.to_string_lossy().into_owned())
            } else {
                let dir = std::env::var_os("XDG_RUNTIME_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(std::env::temp_dir);
                Self(dir.join(format!("brok-{id:016x}.sock")).to_string_lossy().into_owned())
            }
        }
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable 64-bit FNV-1a hash of the workspace path.
fn workspace_id(root: &Path) -> u64 {
    let raw = root.to_string_lossy();
    // Windows paths are case-insensitive.
    let normalized = if cfg!(windows) { raw.to_lowercase() } else { raw.into_owned() };
    normalized
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

/// Connects to a bus.
pub async fn connect(endpoint: &Endpoint) -> io::Result<BoxedStream> {
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(endpoint.as_str()).await?;
        Ok(Box::new(stream))
    }
    #[cfg(windows)]
    {
        use std::time::{Duration, Instant};
        use tokio::net::windows::named_pipe::ClientOptions;

        // All pipe instances are momentarily busy while the server re-arms.
        const ERROR_PIPE_BUSY: i32 = 231;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match ClientOptions::new().open(endpoint.as_str()) {
                Ok(client) => return Ok(Box::new(client)),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Returns `true` when something is accepting connections on `endpoint`.
pub async fn is_listening(endpoint: &Endpoint) -> bool {
    connect(endpoint).await.is_ok()
}

/// Server side of the transport.
pub struct Listener {
    endpoint: Endpoint,
    #[cfg(windows)]
    next: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
}

impl fmt::Debug for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener").field("endpoint", &self.endpoint).finish()
    }
}

impl Listener {
    /// Binds the endpoint. Fails with [`io::ErrorKind::AddrInUse`] when
    /// another runtime already serves it.
    pub fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let path = Path::new(endpoint.as_str());
            if path.exists() {
                if std::os::unix::net::UnixStream::connect(path).is_ok() {
                    return Err(io::Error::new(io::ErrorKind::AddrInUse, "another brok runtime is already listening"));
                }
                std::fs::remove_file(path)?; // stale socket from a crashed runtime
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let inner = tokio::net::UnixListener::bind(path)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            Ok(Self { endpoint: endpoint.clone(), inner })
        }
        #[cfg(windows)]
        {
            let next = Self::create_pipe(endpoint, true).map_err(|e| {
                if e.kind() == io::ErrorKind::PermissionDenied {
                    io::Error::new(io::ErrorKind::AddrInUse, "another brok runtime is already listening")
                } else {
                    e
                }
            })?;
            Ok(Self { endpoint: endpoint.clone(), next })
        }
    }

    #[cfg(windows)]
    fn create_pipe(endpoint: &Endpoint, first: bool) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create(endpoint.as_str())
    }

    /// The bound endpoint.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Waits for the next client.
    pub async fn accept(&mut self) -> io::Result<BoxedStream> {
        #[cfg(unix)]
        {
            let (stream, _) = self.inner.accept().await?;
            Ok(Box::new(stream))
        }
        #[cfg(windows)]
        {
            self.next.connect().await?;
            // Arm a fresh instance before handing out the connected one so
            // there is never a window where clients get "pipe not found".
            let fresh = Self::create_pipe(&self.endpoint, false)?;
            let connected = std::mem::replace(&mut self.next, fresh);
            Ok(Box::new(connected))
        }
    }
}

#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.endpoint.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_stable_per_workspace() {
        let a = Endpoint::for_workspace(Path::new("/tmp/ws-a"));
        assert_eq!(a, Endpoint::for_workspace(Path::new("/tmp/ws-a")));
        assert_ne!(a, Endpoint::for_workspace(Path::new("/tmp/ws-b")));
    }
}
