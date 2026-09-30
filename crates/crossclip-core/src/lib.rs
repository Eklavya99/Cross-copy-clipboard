//! Platform-independent core of CrossClip.
//!
//! Everything that does not touch the desktop (the Cross Clipboard state, wire
//! protocol, TLS transport, discovery and pairing) lives here so that it can be
//! shared by the Windows and Linux agents and tested on any OS.

/// Crate version, reported in the protocol `HELLO` frame and by `crossclip --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
