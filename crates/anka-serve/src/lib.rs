//! HTTP API library used by local `anka-serve` and remote `anka-server`.

pub mod api;

pub use api::{router, AppState, Shared};
