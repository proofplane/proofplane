use std::{net::SocketAddr, sync::Arc, time::Duration};

use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use crate::object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore, StorageError};

const INSTREAM_COMMAND: &[u8] = b"zINSTREAM\0";
const INSTREAM_CHUNK_SIZE: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanObjectRequest {
    pub object_key: ObjectKey,
    pub content_type: String,
    pub content_length: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalwareScanResult {
    pub scanner_name: String,
    pub scanner_version: Option<String>,
    pub outcome: MalwareScanOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MalwareScanOutcome {
    Clean,
    Malicious { reason: String },
    Failed { reason: String },
}

#[derive(Debug, Error)]
pub enum MalwareScanError {
    #[error("object was not found in storage")]
    ObjectNotFound,

    #[error("malware scanner is unavailable: {reason}")]
    Unavailable { reason: String },

    #[error("malware scanner timed out: {reason}")]
    Timeout { reason: String },

    #[error("invalid malware scan request: {reason}")]
    InvalidRequest { reason: String },

    #[error("internal malware scanner error: {reason}")]
    Internal { reason: String },
}

#[derive(Debug)]
pub struct ClamAvMalwareScanner {
    object_store: Arc<FilesystemObjectStore>,
    address: SocketAddr,
    connection_timeout: Duration,
    scan_timeout: Duration,
}

impl ClamAvMalwareScanner {
    pub fn new(
        object_store: Arc<FilesystemObjectStore>,
        address: SocketAddr,
        connection_timeout: Duration,
        scan_timeout: Duration,
    ) -> Self {
        Self {
            object_store,
            address,
            connection_timeout,
            scan_timeout,
        }
    }

    pub async fn scan_object(
        &self,
        request: ScanObjectRequest,
    ) -> Result<MalwareScanResult, MalwareScanError> {
        let object = self
            .object_store
            .get_object(request.object_key.clone())
            .await
            .map_err(map_storage_error)?;

        if object.metadata.content_type != request.content_type
            || object.metadata.content_length != request.content_length
            || object.metadata.sha256 != request.sha256
        {
            return Err(MalwareScanError::InvalidRequest {
                reason: "stored object metadata does not match attachment metadata".to_owned(),
            });
        }

        let mut stream = timeout(self.connection_timeout, TcpStream::connect(self.address))
            .await
            .map_err(|_| MalwareScanError::Timeout {
                reason: format!(
                    "connection to clamd at {} exceeded {:?}",
                    self.address, self.connection_timeout
                ),
            })?
            .map_err(|error| MalwareScanError::Unavailable {
                reason: format!("failed to connect to clamd at {}: {error}", self.address),
            })?;

        let response = timeout(self.scan_timeout, scan_bytes(&mut stream, &object.bytes))
            .await
            .map_err(|_| MalwareScanError::Timeout {
                reason: format!("clamd scan exceeded {:?}", self.scan_timeout),
            })?
            .map_err(|error| MalwareScanError::Unavailable {
                reason: format!("clamd protocol I/O failed: {error}"),
            })?;

        parse_response(&response)
    }
}

async fn scan_bytes(stream: &mut TcpStream, bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    stream.write_all(INSTREAM_COMMAND).await?;
    for chunk in bytes.chunks(INSTREAM_CHUNK_SIZE) {
        stream
            .write_all(
                &u32::try_from(chunk.len())
                    .expect("INSTREAM chunks fit in u32")
                    .to_be_bytes(),
            )
            .await?;
        stream.write_all(chunk).await?;
    }
    stream.write_all(&0_u32.to_be_bytes()).await?;
    stream.flush().await?;

    let mut response = Vec::new();
    loop {
        if response.len() >= MAX_RESPONSE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "clamd response exceeded maximum length",
            ));
        }

        let mut buffer = [0_u8; 1024];
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..read]);
        if response.contains(&0) {
            break;
        }
    }

    Ok(response)
}

fn parse_response(response: &[u8]) -> Result<MalwareScanResult, MalwareScanError> {
    let response = response
        .split(|byte| *byte == 0)
        .next()
        .and_then(|response| std::str::from_utf8(response).ok())
        .map(str::trim)
        .filter(|response| !response.is_empty())
        .ok_or_else(|| MalwareScanError::Internal {
            reason: "clamd returned an empty or invalid response".to_owned(),
        })?;

    let outcome = if response.ends_with(" OK") {
        MalwareScanOutcome::Clean
    } else if let Some(reason) = response.strip_suffix(" FOUND") {
        MalwareScanOutcome::Malicious {
            reason: response_reason(reason),
        }
    } else if let Some(reason) = response.strip_suffix(" ERROR") {
        MalwareScanOutcome::Failed {
            reason: response_reason(reason),
        }
    } else {
        return Err(MalwareScanError::Internal {
            reason: format!("unexpected clamd response: {response}"),
        });
    };

    Ok(MalwareScanResult {
        scanner_name: "clamav".to_owned(),
        scanner_version: None,
        outcome,
    })
}

fn response_reason(response: &str) -> String {
    response
        .split_once(": ")
        .map(|(_, reason)| reason)
        .unwrap_or(response)
        .to_owned()
}

fn map_storage_error(error: StorageError) -> MalwareScanError {
    match error {
        StorageError::NotFound => MalwareScanError::ObjectNotFound,
        error => MalwareScanError::Internal {
            reason: format!("failed to load object from storage: {error}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_malicious_and_failed_responses() {
        assert_eq!(
            parse_response(b"stream: OK\0").unwrap().outcome,
            MalwareScanOutcome::Clean
        );
        assert_eq!(
            parse_response(b"stream: Win.Test.EICAR_HDB-1 FOUND\0")
                .unwrap()
                .outcome,
            MalwareScanOutcome::Malicious {
                reason: "Win.Test.EICAR_HDB-1".to_owned(),
            }
        );
        assert_eq!(
            parse_response(b"stream: INSTREAM size limit exceeded. ERROR\0")
                .unwrap()
                .outcome,
            MalwareScanOutcome::Failed {
                reason: "INSTREAM size limit exceeded.".to_owned(),
            }
        );
    }

    #[test]
    fn rejects_unexpected_responses() {
        assert!(matches!(
            parse_response(b"stream: UNKNOWN\0"),
            Err(MalwareScanError::Internal { .. })
        ));
        assert!(matches!(
            parse_response(b""),
            Err(MalwareScanError::Internal { .. })
        ));
    }
}
