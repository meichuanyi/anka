//! HTTP API library used by local `anka-serve` and remote `anka-server`.

pub mod api;

pub use api::{app_update_router, router, AppState, Shared};
