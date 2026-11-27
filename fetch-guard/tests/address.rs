//! The address vectors of stophammer `tests/adr0054_fetch_rule_tests.rs` and
//! `tests/ssrf_redirect_tests.rs` (stophammer commit 563725e), for
//! `is_public_ip` and `check_url` (stophammer ADR 0054 §1).

use std::net::IpAddr;

use musicindex_fetch_guard::{GuardError, check_url, is_public_ip};
use url::Url;

fn ip(text: &str) -> IpAddr {
    text.parse().expect("address")
}

fn url(text: &str) -> Url {
    Url::parse(text).expect("URL")
}

#[test]
fn is_public_ip_refuses_each_reserved_range() {
    for addr in [
        "127.0.0.1",
        "::1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.0.1",
        "169.254.169.254",
        "100.64.0.1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "64:ff9b::a00:1",
        "192.0.2.1",
        "198.51.100.1",
        "203.0.113.1",
        "198.18.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "0.0.0.0",
        "::",
        "2001:db8::1",
        "::ffff:10.0.0.1",
        "::127.0.0.1",
        "64:ff9b::7f00:1",
    ] {
        assert!(!is_public_ip(ip(addr)), "{addr} must not be public");
    }
}

#[test]
fn is_public_ip_accepts_public_addresses() {
    for addr in [
        "93.184.216.34",
        "100.128.0.1",
        "198.20.0.1",
        "::ffff:1.1.1.1",
        "1.1.1.1",
    ] {
        assert!(is_public_ip(ip(addr)), "{addr} must be public");
    }
}

#[test]
fn check_url_refuses_private_addresses() {
    for text in [
        "http://127.0.0.1/feed.xml",
        "https://127.0.0.1/feed.xml",
        "http://10.0.0.1/feed.xml",
        "http://192.168.1.1/feed.xml",
        "http://169.254.169.254/latest/meta-data/",
        "http://172.16.0.1/feed.xml",
        "http://[::1]/feed.xml",
        "http://[::ffff:10.0.0.1]/feed.xml",
    ] {
        assert!(
            matches!(check_url(&url(text)), Err(GuardError::Address(_))),
            "{text}"
        );
    }
}

#[test]
fn check_url_refuses_localhost_by_name() {
    assert!(matches!(
        check_url(&url("http://localhost:1/")),
        Err(GuardError::Address(_))
    ));
}

#[test]
fn check_url_refuses_other_schemes() {
    for text in ["file:///etc/passwd", "ftp://example.com/feed.xml"] {
        assert!(
            matches!(check_url(&url(text)), Err(GuardError::Scheme(_))),
            "{text}"
        );
    }
}

/// stophammer ADR 0054 §1: the URL has no user name and no password.
#[test]
fn check_url_refuses_credentials() {
    for text in [
        "http://user:pass@93.184.216.34/",
        "http://user@93.184.216.34/",
    ] {
        assert_eq!(
            check_url(&url(text)),
            Err(GuardError::Credentials),
            "{text}"
        );
    }
}

#[test]
fn check_url_accepts_a_public_address() {
    let checked = check_url(&url("https://1.1.1.1/feed.xml")).expect("public");
    assert_eq!(checked.host, "1.1.1.1");
    assert_eq!(checked.addrs, vec!["1.1.1.1:443".parse().expect("addr")]);

    let checked = check_url(&url("https://93.184.216.34:8443/")).expect("public");
    assert_eq!(
        checked.addrs,
        vec!["93.184.216.34:8443".parse().expect("addr")]
    );
}
