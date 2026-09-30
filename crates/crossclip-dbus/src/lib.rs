//! D-Bus service (`io.github.eklavya99.CrossClip`) that the GNOME Shell extension
//! talks to. Linux only; the crate is empty elsewhere.
#![cfg(target_os = "linux")]

/// Well-known bus name owned by the agent.
pub const BUS_NAME: &str = "io.github.eklavya99.CrossClip";
/// Object path of the agent interface.
pub const OBJECT_PATH: &str = "/io/github/eklavya99/CrossClip";
