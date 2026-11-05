//! Everything the `ccterm` processes share about talking to the daemon
//! (DESIGN.md §3.5–3.9): the product name, where sockets and state live, the
//! wire types and their framing, the local transport (Unix socket / named
//! pipe) and a client that mirrors `ccterm_core::SessionHandle` over it.

pub mod client;
pub mod paths;
pub mod product;
pub mod transport;
pub mod wire;

pub use client::{Client, ConnectError, RemoteHandle, RemoteSession};
pub use wire::{ClientMsg, DaemonMsg, Origin, Role, SessionInfo, SessionState};
