use rmcp::{
    model::{Implementation, ServerCapabilities, ServerInfo},
    ServerHandler,
};

use crate::VERSION;

#[derive(Debug, Clone, Default)]
pub struct ProofplaneMcp;

impl ServerHandler for ProofplaneMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().build())
            .with_server_info(Implementation::new("proofplane", VERSION))
    }
}
