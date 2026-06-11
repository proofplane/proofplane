use std::{net::SocketAddr, pin::pin, time::Duration};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

const INSTREAM_COMMAND: &[u8] = b"zINSTREAM\0";
const INSTREAM_CHUNK_SIZE: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

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
    #[error("malware scanner is unavailable: {reason}")]
    Unavailable { reason: String },

    #[error("malware scanner timed out: {reason}")]
    Timeout { reason: String },

    #[error("internal malware scanner error: {reason}")]
    Internal { reason: String },
}

#[derive(Debug)]
pub struct ClamAvMalwareScanner {
    address: SocketAddr,
    connection_timeout: Duration,
    scan_timeout: Duration,
}

impl ClamAvMalwareScanner {
    pub fn new(address: SocketAddr, connection_timeout: Duration, scan_timeout: Duration) -> Self {
        Self {
            address,
            connection_timeout,
            scan_timeout,
        }
    }

    pub async fn scan<S>(&self, chunks: S) -> Result<MalwareScanResult, MalwareScanError>
    where
        S: Stream<Item = Result<Bytes, MalwareScanError>> + Send,
    {
        let connection_attempt =
            timeout(self.connection_timeout, TcpStream::connect(self.address)).await;
        let connection = connection_attempt.map_err(|_| MalwareScanError::Timeout {
            reason: format!(
                "connection to clamd at {} exceeded {:?}",
                self.address, self.connection_timeout
            ),
        })?;
        let mut stream = connection.map_err(|error| MalwareScanError::Unavailable {
            reason: format!("failed to connect to clamd at {}: {error}", self.address),
        })?;

        let scan_attempt = timeout(self.scan_timeout, scan_chunks(&mut stream, chunks)).await;
        let scan_result = scan_attempt.map_err(|_| MalwareScanError::Timeout {
            reason: format!("clamd scan exceeded {:?}", self.scan_timeout),
        })?;
        let response = scan_result?;

        parse_response(&response)
    }
}

async fn scan_chunks<S>(stream: &mut TcpStream, chunks: S) -> Result<Vec<u8>, MalwareScanError>
where
    S: Stream<Item = Result<Bytes, MalwareScanError>>,
{
    stream
        .write_all(INSTREAM_COMMAND)
        .await
        .map_err(protocol_error)?;
    let mut chunks = pin!(chunks);
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        for frame in chunk.chunks(INSTREAM_CHUNK_SIZE) {
            let frame_length =
                u32::try_from(frame.len()).map_err(|_| MalwareScanError::Internal {
                    reason: "INSTREAM frame length exceeds the protocol limit".to_owned(),
                })?;
            stream
                .write_all(&frame_length.to_be_bytes())
                .await
                .map_err(protocol_error)?;
            stream.write_all(frame).await.map_err(protocol_error)?;
        }
    }
    stream
        .write_all(&0_u32.to_be_bytes())
        .await
        .map_err(protocol_error)?;
    stream.flush().await.map_err(protocol_error)?;

    let mut response = Vec::new();
    loop {
        if response.len() >= MAX_RESPONSE_BYTES {
            return Err(MalwareScanError::Internal {
                reason: "clamd response exceeded maximum length".to_owned(),
            });
        }

        let mut buffer = [0_u8; 1024];
        let read = stream.read(&mut buffer).await.map_err(protocol_error)?;
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

fn protocol_error(error: std::io::Error) -> MalwareScanError {
    MalwareScanError::Unavailable {
        reason: format!("clamd protocol I/O failed: {error}"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use tokio::net::TcpListener;

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

    #[tokio::test]
    async fn splits_oversized_input_chunks_into_protocol_frames() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut command = [0_u8; INSTREAM_COMMAND.len()];
            stream.read_exact(&mut command).await.unwrap();
            assert_eq!(&command, INSTREAM_COMMAND);

            let mut frame_lengths = Vec::new();
            loop {
                let length = stream.read_u32().await.unwrap() as usize;
                if length == 0 {
                    break;
                }
                frame_lengths.push(length);
                let mut frame = vec![0_u8; length];
                stream.read_exact(&mut frame).await.unwrap();
            }
            stream.write_all(b"stream: OK\0").await.unwrap();
            frame_lengths
        });
        let scanner =
            ClamAvMalwareScanner::new(address, Duration::from_secs(1), Duration::from_secs(1));

        let result = scanner
            .scan(stream::iter([Ok(Bytes::from(vec![
                0_u8;
                INSTREAM_CHUNK_SIZE + 3
            ]))]))
            .await
            .unwrap();

        assert_eq!(result.outcome, MalwareScanOutcome::Clean);
        assert_eq!(server.await.unwrap(), vec![INSTREAM_CHUNK_SIZE, 3]);
    }

    #[tokio::test]
    async fn propagates_input_stream_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut command = [0_u8; INSTREAM_COMMAND.len()];
            stream.read_exact(&mut command).await.unwrap();
        });
        let scanner =
            ClamAvMalwareScanner::new(address, Duration::from_secs(1), Duration::from_secs(1));
        let input_error = MalwareScanError::Internal {
            reason: "injected storage read failure".to_owned(),
        };

        let result = scanner.scan(stream::iter([Err(input_error)])).await;

        assert!(matches!(
            result,
            Err(MalwareScanError::Internal { reason })
                if reason == "injected storage read failure"
        ));
        server.await.unwrap();
    }
}
