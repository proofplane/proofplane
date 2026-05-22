use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use tonic::transport::{Channel, Endpoint};
use url::Url;

use crate::config::SpiceDbConfig;

// The generated rust code for the protos fails these lints. This is generated
// code so we don't care.
#[allow(dead_code, clippy::enum_variant_names, clippy::large_enum_variant)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/spicedb.rs"));
}

#[derive(Debug, Clone)]
pub struct SpiceDbClient {
    _channel: Channel,
    preshared_key: SecretString,
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("invalid SpiceDB gRPC endpoint {endpoint}: {source}")]
    Endpoint {
        endpoint: Url,
        #[source]
        source: tonic::transport::Error,
    },
    #[error("failed to connect to SpiceDB at {endpoint}: {source}")]
    Connect {
        endpoint: Url,
        #[source]
        source: tonic::transport::Error,
    },
}

impl SpiceDbClient {
    pub async fn from_config(config: &SpiceDbConfig) -> Result<Self, ClientError> {
        let endpoint = Endpoint::from_shared(config.endpoint.to_string()).map_err(|source| {
            ClientError::Endpoint {
                endpoint: config.endpoint.clone(),
                source,
            }
        })?;

        let channel = endpoint
            .connect()
            .await
            .map_err(|source| ClientError::Connect {
                endpoint: endpoint
                    .uri()
                    .to_string()
                    .parse()
                    .expect("validated Tonic endpoint URI stays a URL"),
                source,
            })?;

        Ok(Self {
            _channel: channel,
            preshared_key: config.preshared_key.clone(),
        })
    }

    pub fn preshared_key_is_configured(&self) -> bool {
        !self.preshared_key.expose_secret().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;

    #[tokio::test]
    async fn constructs_from_validated_config() {
        let config = SpiceDbConfig {
            endpoint: Url::parse("http://127.0.0.1:50051").expect("endpoint parses"),
            preshared_key: SecretString::from("test-key"),
        };

        let client = SpiceDbClient::from_config(&config)
            .await
            .expect("client constructs");

        assert!(client.preshared_key_is_configured());
    }
}
