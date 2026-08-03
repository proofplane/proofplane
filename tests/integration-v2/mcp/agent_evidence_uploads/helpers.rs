use super::*;

pub(super) struct MachineTransfer {
    pub(super) upload_id: Uuid,
    pub(super) submission_id: Uuid,
    pub(super) path: String,
    pub(super) authorization: String,
    pub(super) content_type: String,
}

pub(super) struct HttpResult {
    pub(super) status: StatusCode,
    pub(super) body: Value,
}

#[track_caller]
pub(super) fn machine_transfer(prepared: &Value, expected_content_type: &str) -> MachineTransfer {
    assert_eq!(
        object_keys(prepared),
        ["submission_id", "upload", "upload_id"]
            .into_iter()
            .collect()
    );
    assert_eq!(
        object_keys(&prepared["upload"]),
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
    let upload_id = uuid_at(&prepared["upload_id"], "upload id");
    let submission_id = uuid_at(&prepared["submission_id"], "submission id");
    assert_eq!(prepared["upload"]["method"], "PUT");
    assert_eq!(prepared["upload"]["content_type"], expected_content_type);
    assert_eq!(prepared["upload"]["max_bytes"], MAX_DOCUMENT_BYTES);
    assert_rfc3339(&prepared["upload"]["expires_at"]);
    let authorization = prepared["upload"]["authorization"]
        .as_str()
        .expect("upload authorization is text")
        .to_owned();
    let credential = authorization
        .strip_prefix("Proofplane-Upload ")
        .expect("upload authorization uses the machine-transfer scheme");
    assert!(credential.len() > 1);
    let url = url::Url::parse(
        prepared["upload"]["url"]
            .as_str()
            .expect("upload URL is text"),
    )
    .expect("upload URL parses");
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("api.proofplane.test"));
    assert_eq!(url.path(), format!("/agent-evidence-uploads/{upload_id}"));
    assert_eq!(url.query(), None);
    assert_eq!(url.fragment(), None);

    MachineTransfer {
        upload_id,
        submission_id,
        path: local_path(url.as_str()),
        authorization,
        content_type: expected_content_type.to_owned(),
    }
}

pub(super) async fn execute_transfer(
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

pub(super) async fn fail_transfer_on_purpose(
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

pub(super) async fn interrupted_transfer(
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

pub(super) fn response_result(response: TestResponse) -> HttpResult {
    HttpResult {
        status: response.status_code(),
        body: response.json(),
    }
}

#[track_caller]
pub(super) fn raw_response_result(response: &[u8]) -> HttpResult {
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

pub(super) async fn assert_preallocated_submission_is_concealed(
    client: &McpClient,
    evidence_id: Uuid,
    submission_id: Uuid,
) {
    let concealed = client
        .call_tool_error(
            "get_evidence_submission",
            json!({ "submission_id": submission_id }),
        )
        .await;
    assert_not_found(&concealed);
    let listed = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(listed, json!({ "submissions": [] }));
}

#[track_caller]
pub(super) fn assert_http_error(
    result: &HttpResult,
    status: StatusCode,
    code: &str,
    message: &str,
    details: Value,
) {
    assert_eq!(result.status, status);
    assert_eq!(
        result.body,
        json!({
            "error": {
                "code": code,
                "message": message,
                "details": details,
            }
        })
    );
}

#[track_caller]
pub(super) fn assert_pending_result(
    result: &HttpResult,
    expected_status: StatusCode,
    descriptor: &MachineTransfer,
) -> Uuid {
    assert_eq!(result.status, expected_status);
    let document_id = uuid_at(&result.body["document_id"], "pending document id");
    assert_eq!(
        result.body,
        json!({
            "submission_id": descriptor.submission_id,
            "document_id": document_id,
            "upload_status": "pending",
        })
    );
    document_id
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
pub(super) fn assert_submission_projection(
    detail: &Value,
    submission_id: Uuid,
    document_id: Uuid,
    evidence_id: Uuid,
    user_id: Uuid,
    connection_id: Uuid,
    filename: &str,
    bytes: &[u8],
    upload_status: &str,
) {
    assert_eq!(
        object_keys(detail),
        ["document", "submission"].into_iter().collect()
    );
    let submission = &detail["submission"];
    assert_eq!(
        object_keys(submission),
        [
            "evidence_id",
            "id",
            "received_at",
            "submitted_by",
            "valid_from",
            "valid_until",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(submission["id"], submission_id.to_string());
    assert_eq!(submission["evidence_id"], evidence_id.to_string());
    assert_eq!(submission["valid_from"], VALID_FROM);
    assert_eq!(submission["valid_until"], VALID_UNTIL);
    assert_rfc3339(&submission["received_at"]);
    assert_eq!(
        object_keys(&submission["submitted_by"]),
        ["agent_connection_id", "user_id"].into_iter().collect()
    );
    assert_eq!(submission["submitted_by"]["user_id"], user_id.to_string());
    assert_eq!(
        submission["submitted_by"]["agent_connection_id"],
        connection_id.to_string()
    );

    let document = &detail["document"];
    assert_eq!(
        object_keys(document),
        [
            "checksum_crc32c",
            "checksum_sha256",
            "content_length",
            "content_type",
            "created_by_user_id",
            "evidence_submission_id",
            "filename",
            "id",
            "upload_status",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(document["id"], document_id.to_string());
    assert_eq!(
        document["evidence_submission_id"],
        submission_id.to_string()
    );
    assert_eq!(document["created_by_user_id"], user_id.to_string());
    assert_eq!(document["filename"], filename);
    assert_eq!(document["content_type"], CONTENT_TYPE);
    assert_eq!(document["content_length"], bytes.len());
    assert_eq!(document["checksum_sha256"], sha256(bytes));
    assert_eq!(
        document["checksum_crc32c"],
        BASE64_STANDARD.encode(crc32c::crc32c(bytes).to_be_bytes())
    );
    assert_eq!(document["upload_status"], upload_status);
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
pub(super) fn assert_upload_audit_event(
    record: &Value,
    request_id: Uuid,
    event_name: &str,
    client_type: &str,
    operation: &str,
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    object_type: &str,
    object_id: Uuid,
    metadata: Value,
) {
    assert_eq!(
        object_keys(record),
        ["fields", "level", "target", "timestamp"]
            .into_iter()
            .collect()
    );
    assert_eq!(record["level"], "INFO");
    assert_eq!(record["target"], "proofplane::audit");
    assert_rfc3339(&record["timestamp"]);
    let fields = &record["fields"];
    assert_eq!(
        object_keys(fields),
        [
            "actor_type",
            "agent_connection_id",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "type",
            "user_id",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(fields["type"], "audit_log");
    uuid_at(&fields["event_id"], "audit event id");
    assert_eq!(fields["event_name"], event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "agent_connection");
    assert_eq!(fields["user_id"], user_id.to_string());
    assert_eq!(fields["agent_connection_id"], connection_id.to_string());
    assert_eq!(fields["client_type"], client_type);
    assert_eq!(fields["operation"], operation);
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    assert_eq!(fields["request_id"], request_id.to_string());
    assert_eq!(fields["object_type"], object_type);
    assert_eq!(fields["object_id"], object_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is serialized JSON")
        )
        .expect("audit metadata parses"),
        metadata
    );
}

pub(super) fn uuid_at(value: &Value, name: &str) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap_or_else(|| panic!("{name} is text")))
        .unwrap_or_else(|_| panic!("{name} is a UUID"))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(super) fn tamper(value: &str) -> String {
    let mut bytes = value.as_bytes().to_vec();
    let index = bytes.len() / 2;
    bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
    String::from_utf8(bytes).expect("tampered authorization remains UTF-8")
}
