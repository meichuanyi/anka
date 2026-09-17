//! MCP protocol helpers + tool implementations (shared by stdio binary and HTTP server).

pub mod protocol;
pub mod tools;

pub use protocol::{handle_message, ClientMessage, RpcResponse, PROTOCOL_VERSION, SERVER_NAME};
pub use tools::{call_tool_wrapped, tool_definitions};
