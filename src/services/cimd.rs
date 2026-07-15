//! Client ID Metadata Document (CIMD) resolution.
//!
//! MCP clients identify themselves with a stable HTTPS URL (the `client_id`)
//! that resolves to a JSON metadata document describing the client. This
//! replaces Dynamic Client Registration: instead of minting an opaque id per
//! registration, the authorization server fetches the client's own published
//! metadata on demand. The URL is a stable identity, which is what lets the
//! agent-connection reuse machinery deduplicate correctly.
//!
//! Fetching an attacker-influenced URL is an SSRF sink, so resolution is
//! deliberately strict: HTTPS only, DNS resolved and pinned to non-private
//! addresses, redirects refused, body size and time bounded.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::redirect::Policy;
use serde::Deserialize;
use thiserror::Error;
use url::{Host, Url};

/// Maximum size of a metadata document we will read, in bytes.
const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
/// Total timeout for a single metadata fetch.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// Fallback display name when the document omits `client_name`.
const DEFAULT_CLIENT_NAME: &str = "MCP client";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClient {
    pub client_name: String,
    pub redirect_uris: Vec<String>,
}

#[derive(Debug, Error)]
pub enum CimdError {
    /// The `client_id` was not a usable HTTPS metadata URL, or the document it
    /// pointed at was missing, malformed, or internally inconsistent.
    #[error("client id metadata document is invalid")]
    InvalidClient,
    /// The document host resolved only to addresses we refuse to contact.
    #[error("client id metadata document host is not permitted")]
    ForbiddenHost,
    /// The document could not be fetched (network, TLS, timeout, size).
    #[error("client id metadata document could not be fetched")]
    Unreachable,
}

#[derive(Debug, Deserialize)]
struct MetadataDocument {
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_name: Option<String>,
    #[serde(default)]
    redirect_uris: Vec<String>,
}

#[derive(Clone)]
pub struct CimdResolver {
    timeout: Duration,
    max_bytes: usize,
}

impl Default for CimdResolver {
    fn default() -> Self {
        Self {
            timeout: FETCH_TIMEOUT,
            max_bytes: MAX_DOCUMENT_BYTES,
        }
    }
}

impl CimdResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch and validate the metadata document identified by `client_id`.
    ///
    /// `client_id` must be an HTTPS URL. The host is resolved up front and the
    /// connection is pinned to the validated address so a later DNS answer
    /// cannot swap in a forbidden target (DNS rebinding).
    pub async fn resolve(&self, client_id: &str) -> Result<ResolvedClient, CimdError> {
        let url = parse_https_url(client_id)?;
        let addr = self.resolve_permitted_addr(&url).await?;

        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .connect_timeout(self.timeout)
            .timeout(self.timeout)
            .no_proxy()
            .resolve(url.host_str().unwrap_or_default(), addr)
            .build()
            .map_err(|_| CimdError::Unreachable)?;

        fetch_document(&client, &url, self.max_bytes).await
    }

    /// Resolve the URL host to socket addresses and return the first one we are
    /// willing to contact, rejecting private / loopback / link-local ranges.
    async fn resolve_permitted_addr(&self, url: &Url) -> Result<SocketAddr, CimdError> {
        let host = url.host_str().ok_or(CimdError::InvalidClient)?;
        let port = url.port_or_known_default().unwrap_or(443);

        // A literal IP in the URL is validated directly; a name is resolved.
        if let Some(Host::Ipv4(ip)) = url.host() {
            return permit_addr(SocketAddr::new(IpAddr::V4(ip), port));
        }
        if let Some(Host::Ipv6(ip)) = url.host() {
            return permit_addr(SocketAddr::new(IpAddr::V6(ip), port));
        }

        let mut addrs = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| CimdError::Unreachable)?;
        addrs
            .find_map(|addr| permit_addr(addr).ok())
            .ok_or(CimdError::ForbiddenHost)
    }
}

fn parse_https_url(client_id: &str) -> Result<Url, CimdError> {
    let url = Url::parse(client_id).map_err(|_| CimdError::InvalidClient)?;
    if url.scheme() != "https" {
        return Err(CimdError::InvalidClient);
    }
    if url.host().is_none() {
        return Err(CimdError::InvalidClient);
    }
    Ok(url)
}

fn permit_addr(addr: SocketAddr) -> Result<SocketAddr, CimdError> {
    if is_forbidden_ip(addr.ip()) {
        return Err(CimdError::ForbiddenHost);
    }
    Ok(addr)
}

/// Reject addresses that are not safely routable on the public internet.
/// `IpAddr::is_global` is still unstable, so the ranges are checked directly.
fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_forbidden_ipv4(mapped);
            }
            is_forbidden_ipv6(v6)
        }
    }
}

fn is_forbidden_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Carrier-grade NAT shared address space (100.64.0.0/10).
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 0x40)
        // IETF benchmarking (198.18.0.0/15).
        || (ip.octets()[0] == 198 && (ip.octets()[1] & 0xfe) == 18)
}

fn is_forbidden_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Unique local addresses (fc00::/7).
        || (ip.segments()[0] & 0xfe00) == 0xfc00
        // Link-local unicast (fe80::/10).
        || (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Fetch, size-bound, parse, and validate the metadata document at `url` using
/// an already-configured client. Split out from address validation so the wire
/// behaviour can be exercised against a local test server.
async fn fetch_document(
    client: &reqwest::Client,
    url: &Url,
    max_bytes: usize,
) -> Result<ResolvedClient, CimdError> {
    let response = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| CimdError::Unreachable)?;
    if !response.status().is_success() {
        return Err(CimdError::InvalidClient);
    }
    let body = read_capped_body(response, max_bytes).await?;
    let document: MetadataDocument =
        serde_json::from_slice(&body).map_err(|_| CimdError::InvalidClient)?;
    validate_document(url, document)
}

async fn read_capped_body(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, CimdError> {
    if let Some(len) = response.content_length() {
        if len > max_bytes as u64 {
            return Err(CimdError::InvalidClient);
        }
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| CimdError::Unreachable)? {
        if body.len() + chunk.len() > max_bytes {
            return Err(CimdError::InvalidClient);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn validate_document(url: &Url, document: MetadataDocument) -> Result<ResolvedClient, CimdError> {
    // If the document names its own client_id it must be the URL we fetched.
    if let Some(declared) = document.client_id.as_deref() {
        if declared != url.as_str() {
            return Err(CimdError::InvalidClient);
        }
    }
    if document.redirect_uris.is_empty() {
        return Err(CimdError::InvalidClient);
    }
    let client_name = document
        .client_name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| DEFAULT_CLIENT_NAME.to_owned());
    Ok(ResolvedClient {
        client_name,
        redirect_uris: document.redirect_uris,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_https_client_ids() {
        for client_id in [
            "http://example.com/client.json",
            "ftp://example.com/client.json",
            "not a url",
            "https://",
        ] {
            assert!(matches!(
                parse_https_url(client_id),
                Err(CimdError::InvalidClient)
            ));
        }
    }

    #[test]
    fn forbids_private_and_loopback_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "198.18.0.1",
            "0.0.0.0",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
        ] {
            let ip: IpAddr = ip.parse().expect("ip parses");
            assert!(is_forbidden_ip(ip), "expected {ip} to be forbidden");
        }
    }

    #[test]
    fn permits_public_addresses() {
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            let ip: IpAddr = ip.parse().expect("ip parses");
            assert!(!is_forbidden_ip(ip), "expected {ip} to be permitted");
        }
    }

    #[test]
    fn document_client_id_must_match_url() {
        let url = Url::parse("https://client.example/meta.json").expect("url parses");
        let mismatched = MetadataDocument {
            client_id: Some("https://evil.example/meta.json".to_owned()),
            client_name: Some("Client".to_owned()),
            redirect_uris: vec!["https://client.example/cb".to_owned()],
        };
        assert!(matches!(
            validate_document(&url, mismatched),
            Err(CimdError::InvalidClient)
        ));
    }

    #[test]
    fn document_requires_redirect_uris() {
        let url = Url::parse("https://client.example/meta.json").expect("url parses");
        let empty = MetadataDocument {
            client_id: None,
            client_name: Some("Client".to_owned()),
            redirect_uris: vec![],
        };
        assert!(matches!(
            validate_document(&url, empty),
            Err(CimdError::InvalidClient)
        ));
    }

    #[test]
    fn document_defaults_missing_client_name() {
        let url = Url::parse("https://client.example/meta.json").expect("url parses");
        let resolved = validate_document(
            &url,
            MetadataDocument {
                client_id: Some(url.as_str().to_owned()),
                client_name: None,
                redirect_uris: vec!["https://client.example/cb".to_owned()],
            },
        )
        .expect("document validates");
        assert_eq!(resolved.client_name, DEFAULT_CLIENT_NAME);
        assert_eq!(resolved.redirect_uris, ["https://client.example/cb"]);
    }

    #[tokio::test]
    async fn resolve_rejects_non_https_before_any_fetch() {
        let resolver = CimdResolver::new();
        assert!(matches!(
            resolver.resolve("http://example.com/client.json").await,
            Err(CimdError::InvalidClient)
        ));
    }

    #[tokio::test]
    async fn resolve_rejects_literal_private_ip_host() {
        let resolver = CimdResolver::new();
        for client_id in [
            "https://127.0.0.1/client.json",
            "https://10.0.0.1/client.json",
            "https://[::1]/client.json",
        ] {
            assert!(
                matches!(
                    resolver.resolve(client_id).await,
                    Err(CimdError::ForbiddenHost)
                ),
                "expected {client_id} to be forbidden"
            );
        }
    }

    // Wire-level coverage: exercise fetch_document against a local server. The
    // SSRF address guard is validated separately, so these use an ordinary
    // client pointed straight at the loopback test server.
    async fn serve(status: u16, body: &'static str) -> (Url, tokio::task::JoinHandle<()>) {
        use axum::{routing::get, Router};
        let app = Router::new().route(
            "/client.json",
            get(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).expect("status"),
                    body,
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binds");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serves");
        });
        let url = Url::parse(&format!("http://{addr}/client.json")).expect("url");
        (url, handle)
    }

    #[tokio::test]
    async fn fetch_document_resolves_valid_metadata() {
        let (url, handle) = serve(
            200,
            r#"{"client_name":"Test Client","redirect_uris":["https://client.example/cb"]}"#,
        )
        .await;
        let client = reqwest::Client::new();
        let resolved = fetch_document(&client, &url, MAX_DOCUMENT_BYTES)
            .await
            .expect("document resolves");
        assert_eq!(resolved.client_name, "Test Client");
        assert_eq!(resolved.redirect_uris, ["https://client.example/cb"]);
        handle.abort();
    }

    #[tokio::test]
    async fn fetch_document_rejects_non_success_status() {
        let (url, handle) = serve(404, "not found").await;
        let client = reqwest::Client::new();
        assert!(matches!(
            fetch_document(&client, &url, MAX_DOCUMENT_BYTES).await,
            Err(CimdError::InvalidClient)
        ));
        handle.abort();
    }

    #[tokio::test]
    async fn fetch_document_rejects_malformed_json() {
        let (url, handle) = serve(200, "{ not json").await;
        let client = reqwest::Client::new();
        assert!(matches!(
            fetch_document(&client, &url, MAX_DOCUMENT_BYTES).await,
            Err(CimdError::InvalidClient)
        ));
        handle.abort();
    }

    #[tokio::test]
    async fn fetch_document_rejects_oversized_body() {
        let (url, handle) = serve(
            200,
            r#"{"client_name":"Test Client","redirect_uris":["https://client.example/cb"]}"#,
        )
        .await;
        let client = reqwest::Client::new();
        assert!(matches!(
            fetch_document(&client, &url, 8).await,
            Err(CimdError::InvalidClient)
        ));
        handle.abort();
    }
}
