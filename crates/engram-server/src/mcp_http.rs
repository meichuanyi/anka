//! Remote MCP: JSON-RPC 2.0 over HTTP POST /mcp
//!
//! Single JSON-RPC object or batch array. Same tool surface as stdio `engram-mcp`.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use engram_mcp::{handle_message, ClientMessage, RpcResponse};
use engram_serve::Shared;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/mcp", post(mcp_endpoint))
        .with_state(state)
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum Body {
    One(ClientMessage),
    Many(Vec<ClientMessage>),
}

async fn mcp_endpoint(
    State(state): State<Shared>,
    Json(body): Json<Body>,
) -> Json<serde_json::Value> {
    match body {
        Body::One(msg) => Json(json_rpc(handle_http(&state, msg))),
        Body::Many(msgs) => {
            let out: Vec<RpcResponse> =
                msgs.into_iter().map(|m| handle_http(&state, m)).collect();
            Json(json_rpc_list(out))
        }
    }
}

fn handle_http(state: &Shared, msg: ClientMessage) -> RpcResponse {
    handle_message(&state.collection, msg)
}

fn json_rpc(resp: RpcResponse) -> serde_json::Value {
    serde_json::to_value(resp).unwrap_or(serde_json::Value::Null)
}

fn json_rpc_list(list: Vec<RpcResponse>) -> serde_json::Value {
    serde_json::to_value(list).unwrap_or(serde_json::Value::Null)
}
