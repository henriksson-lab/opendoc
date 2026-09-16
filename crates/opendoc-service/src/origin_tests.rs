//! The origin allowlist, over a real socket.
//!
//! [`crate::origin`]'s own tests cover the matching. These cover the part a
//! unit test cannot: that the allowlist is actually on the wire, that it wraps
//! the WebSocket upgrade as well as the HTTP API, and that a program sending
//! no `Origin` is unaffected by any of it.
//!
//! Requests here are written as raw HTTP/1.1 rather than through
//! [`crate::client`], because the thing under test is a header that client
//! deliberately never sends.

use crate::origin::OriginPolicy;
use crate::server::{serve, RunningServer};
use crate::service::OpenDocService;
use crate::test_support::{register_default_subjects, TempRoot, ALICE_KEY};
use crate::Clock;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const PAGE_ORIGIN: &str = "http://127.0.0.1:10084";

async fn start(root: &TempRoot, origins: OriginPolicy) -> RunningServer {
    let service =
        Arc::new(OpenDocService::new(root.store(), Clock::system()).with_allowed_origins(origins));
    register_default_subjects(service.identity());
    serve(service, "127.0.0.1:0".parse().unwrap())
        .await
        .expect("binding an ephemeral port")
}

/// One request, written by hand, read back whole.
///
/// `headers` are extra lines; the body is JSON when non-empty. The whole
/// response is returned as text so a test can assert on the status line and on
/// header presence and absence alike — absence being the half a typed client
/// would hide.
async fn raw_request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> String {
    let mut stream = TcpStream::connect(address).await.expect("connect");
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream
        .write_all(request.as_bytes())
        .await
        .expect("writing the request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("reading the response");
    String::from_utf8_lossy(&response).to_string()
}

fn session_body() -> String {
    format!(r#"{{"subject":"alice","api_key":"{ALICE_KEY}"}}"#)
}

/// Lower-cased so an assertion does not depend on hyper's header casing.
fn has_header(response: &str, name: &str) -> bool {
    response
        .to_ascii_lowercase()
        .lines()
        .any(|line| line.starts_with(&format!("{}:", name.to_ascii_lowercase())))
}

#[tokio::test]
async fn a_program_that_sends_no_origin_is_untouched_by_the_allowlist() {
    let root = TempRoot::new("origin-none");
    // Deny-all, which is the default: a client with no `Origin` must still
    // work, or configuring this would break every non-browser caller.
    let server = start(&root, OriginPolicy::deny_all()).await;
    let response = raw_request(
        server.local_address(),
        "POST",
        "/v1/sessions",
        &[],
        &session_body(),
    )
    .await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "a request with no Origin must be answered: {response}"
    );
    assert!(
        !has_header(&response, "access-control-allow-origin"),
        "nothing asked for CORS, so nothing should be sent: {response}"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn a_page_on_an_unlisted_origin_is_refused() {
    let root = TempRoot::new("origin-refused");
    let server = start(&root, OriginPolicy::parse_list(PAGE_ORIGIN)).await;
    let response = raw_request(
        server.local_address(),
        "POST",
        "/v1/sessions",
        &[("Origin", "http://evil.example")],
        &session_body(),
    )
    .await;
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "an unlisted origin must be refused: {response}"
    );
    assert!(
        response.contains("forbidden"),
        "the refusal must carry the wire code: {response}"
    );
    assert!(
        !has_header(&response, "access-control-allow-origin"),
        "a refusal must not hand the page permission anyway: {response}"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn an_allowed_page_gets_the_token_and_the_header_that_lets_it_read_it() {
    let root = TempRoot::new("origin-allowed");
    let server = start(&root, OriginPolicy::parse_list(PAGE_ORIGIN)).await;
    let response = raw_request(
        server.local_address(),
        "POST",
        "/v1/sessions",
        &[("Origin", PAGE_ORIGIN)],
        &session_body(),
    )
    .await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "an allowed origin must be answered: {response}"
    );
    assert!(
        response
            .to_ascii_lowercase()
            .contains(&format!("access-control-allow-origin: {PAGE_ORIGIN}")),
        "the allowed origin must be echoed, not starred: {response}"
    );
    assert!(
        response.to_ascii_lowercase().contains("vary: origin"),
        "the answer depends on the origin and must say so: {response}"
    );
    assert!(
        response.contains("\"token\""),
        "the page must actually get its session: {response}"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn a_preflight_is_answered_rather_than_routed_to_a_method_that_does_not_exist() {
    let root = TempRoot::new("origin-preflight");
    let server = start(&root, OriginPolicy::parse_list(PAGE_ORIGIN)).await;
    let response = raw_request(
        server.local_address(),
        "OPTIONS",
        "/v1/sessions",
        &[
            ("Origin", PAGE_ORIGIN),
            ("Access-Control-Request-Method", "POST"),
            ("Access-Control-Request-Headers", "content-type"),
        ],
        "",
    )
    .await;
    assert!(
        response.starts_with("HTTP/1.1 204"),
        "a preflight must be answered, not 405'd by the router: {response}"
    );
    let lowered = response.to_ascii_lowercase();
    assert!(
        lowered.contains("access-control-allow-methods"),
        "a preflight answer must name the methods: {response}"
    );
    assert!(
        lowered.contains("access-control-allow-headers: authorization, content-type"),
        "a preflight answer must allow the two headers this API needs: {response}"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn a_socket_upgrade_from_an_unlisted_origin_is_refused_before_it_upgrades() {
    let root = TempRoot::new("origin-socket");
    let server = start(&root, OriginPolicy::parse_list(PAGE_ORIGIN)).await;
    // The same-origin policy does not cover a WebSocket handshake, so this is
    // the request a hostile page would make, with a token it somehow obtained.
    // It must not become a socket.
    let response = raw_request(
        server.local_address(),
        "GET",
        "/v1/documents/any-document/socket?token=whatever",
        &[
            ("Origin", "http://evil.example"),
            ("Upgrade", "websocket"),
            ("Connection", "Upgrade"),
            ("Sec-WebSocket-Version", "13"),
            ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
        ],
        "",
    )
    .await;
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "a cross-site socket handshake must be refused: {response}"
    );
    assert!(
        !response.contains("101"),
        "it must never reach the upgrade: {response}"
    );
    server.shutdown().await;
}
