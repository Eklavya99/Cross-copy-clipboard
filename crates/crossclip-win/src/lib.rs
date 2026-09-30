//! Windows desktop integration for CrossClip.
//!
//! [`dib`] and [`keys`] are plain Rust with no Win32 calls, so they build and are
//! tested on every platform. Everything that talks to Win32 is `cfg(windows)`.

pub mod dib;
pub mod keys;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod input;

#[cfg(windows)]
pub use app::run;
