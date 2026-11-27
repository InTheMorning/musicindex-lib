//! The redirect vectors of stophammer `tests/ssrf_redirect_tests.rs`
//! (stophammer commit 563725e), for `GuardedFetch::fetch`.
//!
//! The test server is on the loopback address, so each fetch accepts that
//! one socket address with `accept_address_for_tests`. Each other loopback
//! address and port stays refused. That method exists only in a build with
//! debug assertions.

#![cfg(debug_assertions)]

use std::net::SocketAddr;

use musicindex_fetch_guard::{GuardError, GuardedFetch};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn server_addr(server: &MockServer) -> SocketAddr {
    *server.address()
}

fn guarded(server: &MockServer) -> GuardedFetch {
    GuardedFetch::new("musicindex-fetch-guard-test").accept_address_for_tests(server_addr(server))
}

fn server_url(server: &MockServer) -> Url {
    Url::parse(&server.uri()).expect("server URL")
}

async fn redirect_server(location: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", location))
        .mount(&server)
        .await;
    server
}

async fn fetch_redirect(location: &str) -> Result<Vec<u8>, GuardError> {
    let server = redirect_server(location).await;
    guarded(&server).fetch(&server_url(&server)).await
}

#[tokio::test]
async fn redirect_to_a_private_address_is_blocked() {
    for (location, refused) in [
        ("http://127.0.0.1:9999/secret", "127.0.0.1"),
        ("http://127.0.0.2/secret", "127.0.0.2"),
        (
            "http://169.254.169.254/latest/meta-data/",
            "169.254.169.254",
        ),
        ("http://10.0.0.1:8080/internal", "10.0.0.1"),
        ("http://192.168.1.1/admin", "192.168.1.1"),
        ("http://172.16.0.1/internal", "172.16.0.1"),
    ] {
        assert_eq!(
            fetch_redirect(location).await,
            Err(GuardError::Address(refused.parse().expect("addr"))),
            "{location}"
        );
    }
}

#[tokio::test]
async fn redirect_to_a_file_url_is_blocked() {
    assert_eq!(
        fetch_redirect("file:///etc/passwd").await,
        Err(GuardError::Scheme("file".to_owned()))
    );
}

#[tokio::test]
async fn redirect_with_credentials_is_blocked() {
    let server = MockServer::start().await;
    let location = format!("http://user:pass@{}/next", server_addr(&server));
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", location.as_str()))
        .mount(&server)
        .await;
    assert_eq!(
        guarded(&server).fetch(&server_url(&server)).await,
        Err(GuardError::Credentials)
    );
}

#[tokio::test]
async fn direct_fetch_gives_the_body_bytes() {
    let server = MockServer::start().await;
    let body = b"\xEF\xBB\xBF1\n00:00:01,000 --> 00:00:02,000\nHello\n".to_vec();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    assert_eq!(guarded(&server).fetch(&server_url(&server)).await, Ok(body));
}

#[tokio::test]
async fn relative_redirect_on_the_same_server_is_followed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(ResponseTemplate::new(301).insert_header("Location", "/end"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/end"))
        .respond_with(ResponseTemplate::new(200).set_body_string("done"))
        .mount(&server)
        .await;
    let start = server_url(&server).join("/start").expect("URL");
    assert_eq!(guarded(&server).fetch(&start).await, Ok(b"done".to_vec()));
}

#[tokio::test]
async fn the_redirect_limit_is_enforced() {
    let server = MockServer::start().await;
    let location = format!("{}/other", server.uri());
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", location.as_str()))
        .mount(&server)
        .await;
    for max_redirects in [0, 3] {
        assert_eq!(
            guarded(&server)
                .with_max_redirects(max_redirects)
                .fetch(&server_url(&server))
                .await,
            Err(GuardError::TooManyRedirects(max_redirects))
        );
    }
}

#[tokio::test]
async fn an_oversized_body_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(1024)))
        .mount(&server)
        .await;
    assert_eq!(
        guarded(&server)
            .with_max_body_bytes(100)
            .fetch(&server_url(&server))
            .await,
        Err(GuardError::TooLarge(100))
    );
}

#[tokio::test]
async fn an_error_status_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    assert_eq!(
        guarded(&server).fetch(&server_url(&server)).await,
        Err(GuardError::Status(404))
    );
}

/// With the default rule, the test server on the loopback address is refused
/// before a request.
#[tokio::test]
async fn the_default_rule_refuses_the_test_server() {
    let server = MockServer::start().await;
    assert_eq!(
        GuardedFetch::new("musicindex-fetch-guard-test")
            .fetch(&server_url(&server))
            .await,
        Err(GuardError::Address(server_addr(&server).ip()))
    );
}

/// A chain limit too large for the clock gives no panic.
#[tokio::test]
async fn a_very_large_chain_limit_gives_no_panic() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    assert_eq!(
        guarded(&server)
            .with_timeouts(std::time::Duration::from_secs(10), std::time::Duration::MAX)
            .fetch(&server_url(&server))
            .await,
        Ok(b"ok".to_vec())
    );
}
