//! The match of a `host:` value to the host of a feed URL (musicindex ADR 0062
//! R-QUERY-16).
//!
//! An app with a Library applies `host:` to its Library rows with these
//! functions. The app gives the host of the feed URL, and this module parses no
//! URL. The rule is the rule of Stophammer ADR 0080 section 5.
//!
//! The module has no I/O.

/// `true` when `feed_host` is the host of `value` or a subdomain of it.
///
/// ASCII letters fold, and one trailing dot on each side has no effect. Other
/// letters do not fold. An empty value or an empty feed host matches nothing.
#[must_use]
pub fn host_matches(value: &str, feed_host: &str) -> bool {
    let value = host_key(value);
    let feed_host = host_key(feed_host);
    if value.is_empty() || feed_host.is_empty() {
        return false;
    }
    feed_host == value
        || feed_host
            .strip_suffix(value.as_str())
            .is_some_and(|start| start.ends_with('.'))
}

/// `true` when a Library row stays for one `host:` term (musicindex ADR 0062
/// R-QUERY-16 item 3).
///
/// `excluded` is `true` for `-host:`. `feed_host` is the host of the feed URL
/// of the row, or `None` when the row has no such host. A row with no host does
/// not match `host:`, and `-host:` does not remove it.
#[must_use]
pub fn host_keeps_row(value: &str, excluded: bool, feed_host: Option<&str>) -> bool {
    match feed_host {
        None => excluded,
        Some(feed_host) => host_matches(value, feed_host) != excluded,
    }
}

/// The host in ASCII lowercase, with no trailing dot.
fn host_key(host: &str) -> String {
    host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase()
}
