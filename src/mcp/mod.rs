mod auth;
mod context;
mod server;
mod transport;

pub use context::{McpRequestContext, SESSION_ID_HEADER};
pub use server::ProofplaneMcp;
pub use transport::{
    create_app, protocol_router, McpAppDependencies, ENDPOINT, PROTECTED_RESOURCE_METADATA_ENDPOINT,
};
