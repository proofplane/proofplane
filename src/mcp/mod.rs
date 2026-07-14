mod auth;
mod context;
mod docs;
mod server;
mod transport;

pub use context::{McpRequestContext, SESSION_ID_HEADER};
pub use server::ProofplaneMcp;
pub use transport::{create_app, protocol_router, McpAppDependencies, McpAppError, ENDPOINT};
