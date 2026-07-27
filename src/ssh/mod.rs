//! SSH transport over `gh cs ssh --stdio` subprocess.
//!
//! This is the riskiest module — pure Rust SSH over a subprocess's stdin/stdout.
//! russh 0.46 provides the SSH protocol; we provide the transport by wrapping
//! `tokio::process::ChildStdin` and `ChildStdout` in an `AsyncRead + AsyncWrite`
//! type.
//!
//! Wave 4 subagent: implement the methods declared below.

pub mod exec;
pub mod host_keys;
pub mod transport;

pub use exec::{ExecError, ExecResult};
pub use host_keys::{HostKeyError, HostKeyStore};
pub use transport::{connect_codespace, CodespaceSsh, SshTransport};

/// Configuration for an SSH session.
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub codespace_name: String,
    pub timeout_secs: u64,
    pub accept_new_host_key: bool,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            codespace_name: String::new(),
            timeout_secs: 300,
            accept_new_host_key: false,
        }
    }
}
