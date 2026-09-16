//! HTTP API library used by local `engram-serve` and remote `engram-server`.

pub mod api;

pub use api::{router, AppState, Shared};
