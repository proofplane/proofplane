use axum_test::TestResponse;
use bytes::Bytes;
use http::StatusCode;
use proofplane::routes::request_context::REQUEST_ID_HEADER;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use uuid::Uuid;

use super::{
    harness::TestApp,
    http::local_path,
    json::{assert_rfc3339, object_keys},
};

pub const MAX_DOCUMENT_BYTES: u64 = 25 * 1024 * 1024;

pub struct MachineTransfer {
    pub upload_id: Uuid,
    pub path: String,
    pub authorization: String,
    pub content_type: String,
}

pub struct HttpResult {
    pub status: StatusCode,
    pub body: Value,
}

#[track_caller]
pub fn machine_transfer(
    upload_id: &Value,
    upload: &Value,
    expected_content_type: &str,
    route: &str,
) -> MachineTransfer {
    assert_eq!(
        object_keys(upload),
        [
            "authorization",
            "content_type",
            "expires_at",
            "max_bytes",
            "method",
            "url",
        ]
        .into_iter()
        .collect()
    );
    let upload_id = uuid_at(upload_id, "upload id");
    assert_eq!(upload["method"], "PUT");
    assert_eq!(upload["content_type"], expected_content_type);
    assert_eq!(upload["max_bytes"], MAX_DOCUMENT_BYTES);
    assert_rfc3339(&upload["expires_at"]);
    let authorization = upload["authorization"]
        .as_str()
        .expect("upload authorization is text")
        .to_owned();
    let credential = authorization
        .strip_prefix("Proofplane-Upload ")
        .expect("upload authorization uses the machine-transfer scheme");
    assert!(credential.len() > 1);
    let url = url::Url::parse(upload["url"].as_str().expect("upload URL is text"))
        .expect("upload URL parses");
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("api.proofplane.test"));
    assert_eq!(url.path(), format!("/{route}/{upload_id}"));
    assert_eq!(url.query(), None);
    assert_eq!(url.fragment(), None);

    MachineTransfer {
        upload_id,
        path: local_path(url.as_str()),
        authorization,
        content_type: expected_content_type.to_owned(),
    }
}

pub async fn execute_transfer(
    app: &TestApp,
    descriptor: &MachineTransfer,
    body: &[u8],
    request_id: Uuid,
) -> HttpResult {
    let response = app
        .app_server()
        .put(&descriptor.path)
        .add_header(REQUEST_ID_HEADER, request_id.to_string())
        .add_header("authorization", descriptor.authorization.clone())
        .add_header("content-type", descriptor.content_type.clone())
        .add_header("content-length", body.len().to_string())
        .bytes(Bytes::copy_from_slice(body))
        .await;
    response_result(response)
}

#[allow(clippy::too_many_arguments)]
pub async fn fail_transfer_on_purpose(
    app: &TestApp,
    path: &str,
    authorization: Option<&str>,
    content_type: Option<&str>,
    content_length: Option<u64>,
    body: &[u8],
    request_id: Uuid,
) -> HttpResult {
    let mut request = app
        .app_server()
        .put(path)
        .add_header(REQUEST_ID_HEADER, request_id.to_string());
    if let Some(authorization) = authorization {
        request = request.add_header("authorization", authorization);
    }
    if let Some(content_type) = content_type {
        request = request.add_header("content-type", content_type);
    }
    if let Some(content_length) = content_length {
        request = request.add_header("content-length", content_length.to_string());
    }
    if body.is_empty() {
        response_result(request.await)
    } else {
        response_result(request.bytes(Bytes::copy_from_slice(body)).await)
    }
}

pub async fn interrupted_transfer(
    app: &TestApp,
    descriptor: &MachineTransfer,
    partial_body: &[u8],
    declared_length: usize,
    request_id: Uuid,
) -> HttpResult {
    let address = app
        .app_server()
        .server_address()
        .expect("HTTP test server exposes an address");
    let host = address.host_str().expect("HTTP test server has a host");
    let port = address
        .port_or_known_default()
        .expect("HTTP test server has a port");
    let mut stream = TcpStream::connect((host, port))
        .await
        .expect("raw upload connection opens");
    let head = format!(
        "PUT {} HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: {}\r\nContent-Type: {}\r\nContent-Length: {declared_length}\r\n{}: {request_id}\r\nConnection: close\r\n\r\n",
        descriptor.path,
        descriptor.authorization,
        descriptor.content_type,
        REQUEST_ID_HEADER.as_str(),
    );
    stream
        .write_all(head.as_bytes())
        .await
        .expect("raw upload headers write");
    stream
        .write_all(partial_body)
        .await
        .expect("partial raw upload body writes");
    stream
        .shutdown()
        .await
        .expect("raw upload write side closes");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("raw upload response reads completely");
    raw_response_result(&response)
}

pub fn response_result(response: TestResponse) -> HttpResult {
    HttpResult {
        status: response.status_code(),
        body: response.json(),
    }
}

#[track_caller]
pub fn raw_response_result(response: &[u8]) -> HttpResult {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("raw HTTP response has a header terminator");
    let head = std::str::from_utf8(&response[..split]).expect("raw HTTP headers are UTF-8");
    let status = head
        .lines()
        .next()
        .expect("raw HTTP response has a status line")
        .split_whitespace()
        .nth(1)
        .expect("raw HTTP status line has a code")
        .parse::<u16>()
        .expect("raw HTTP status is numeric");
    let body = serde_json::from_slice(&response[split + 4..])
        .expect("raw HTTP response body is complete JSON");
    HttpResult {
        status: StatusCode::from_u16(status).expect("raw HTTP status is valid"),
        body,
    }
}

pub fn uuid_at(value: &Value, name: &str) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap_or_else(|| panic!("{name} is text")))
        .unwrap_or_else(|_| panic!("{name} is a UUID"))
}

pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn tamper(value: &str) -> String {
    let mut bytes = value.as_bytes().to_vec();
    let index = bytes.len() / 2;
    bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
    String::from_utf8(bytes).expect("tampered authorization remains UTF-8")
}
