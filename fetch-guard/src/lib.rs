//! The address test and the guarded fetch of musicindex ADR 0104 R-LYRIC-9
//! item 2. The rules of the test are stophammer ADR 0054 §1.
//!
//! [`is_public_ip`] is the address test. [`check_url`] examines a URL and
//! gives the checked addresses of its host. With the feature `fetch` (on by
//! default), [`GuardedFetch`] fetches a URL. It examines each hop of a
//! redirect, and it pins the connection of each hop to the addresses that it
//! examined, so a second DNS answer cannot change the address.
//!
//! The crate has no dependency on the other crates of musicindex-lib.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

use url::{Host, Url};

/// The host of a URL that [`check_url`] accepts, with its checked addresses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedHost {
    /// The host name, or the text of the literal address.
    pub host: String,
    /// Each address of the host. Each address is public.
    pub addrs: Vec<SocketAddr>,
}

/// The reason that the guard refuses a URL or a fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GuardError {
    /// The URL could not be parsed.
    BadUrl(String),
    /// The scheme is not `http` or `https`.
    Scheme(String),
    /// The URL has a user name or a password (stophammer ADR 0054 §1).
    Credentials,
    /// The URL has no host.
    NoHost,
    /// The host is, or resolves to, an address that is not public.
    Address(IpAddr),
    /// The DNS lookup of the host failed.
    Dns(String),
    /// The DNS lookup of the host gave no address.
    NoAddress(String),
    /// The redirect chain has more hops than the limit.
    TooManyRedirects(usize),
    /// A redirect response has no usable `Location` header.
    BadLocation(String),
    /// The final response has a status that is not a success.
    Status(u16),
    /// The body is larger than the limit.
    TooLarge(usize),
    /// The redirect chain took more time than the limit.
    Timeout,
    /// The HTTP request failed.
    Request(String),
}

impl fmt::Display for GuardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadUrl(error) => write!(formatter, "invalid URL: {error}"),
            Self::Scheme(scheme) => write!(formatter, "blocked: disallowed URL scheme {scheme}"),
            Self::Credentials => {
                formatter.write_str("blocked: the URL has a user name or a password")
            }
            Self::NoHost => formatter.write_str("blocked: the URL has no host"),
            Self::Address(ip) => write!(formatter, "blocked: private or reserved address {ip}"),
            Self::Dns(error) => write!(formatter, "DNS lookup failed: {error}"),
            Self::NoAddress(host) => write!(formatter, "DNS lookup gave no address for {host}"),
            Self::TooManyRedirects(limit) => write!(formatter, "too many redirects (max {limit})"),
            Self::BadLocation(error) => write!(formatter, "invalid redirect Location: {error}"),
            Self::Status(status) => write!(formatter, "HTTP status {status}"),
            Self::TooLarge(limit) => write!(formatter, "response exceeds {limit} bytes"),
            Self::Timeout => formatter.write_str("redirect chain timed out"),
            Self::Request(error) => write!(formatter, "request failed: {error}"),
        }
    }
}

impl std::error::Error for GuardError {}

/// `true` only for a public unicast address (stophammer ADR 0054 §1).
///
/// Refuses the IPv4 loopback, private, link-local, CGNAT (`100.64.0.0/10`),
/// multicast, broadcast, unspecified, documentation (`192.0.2.0/24`,
/// `198.51.100.0/24`, `203.0.113.0/24`) and benchmark (`198.18.0.0/15`)
/// ranges. Refuses the IPv6 loopback, unspecified, ULA (`fc00::/7`),
/// link-local (`fe80::/10`), multicast (`ff00::/8`) and documentation
/// (`2001:db8::/32`) ranges. An IPv4-mapped (`::ffff:0:0/96`),
/// IPv4-compatible (`::/96`) or NAT64 (`64:ff9b::/96`) address gets the
/// result of the IPv4 address in it.
#[must_use]
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => is_public_ipv6(v6),
    }
}

fn is_public_ipv4(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24 (documentation, RFC 5737)
    let documentation = (o[0] == 192 && o[1] == 0 && o[2] == 2)
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113);
    // 198.18.0.0/15 (benchmark, RFC 2544)
    let benchmark = o[0] == 198 && (o[1] & 0xFE) == 18;
    // 100.64.0.0/10 (CGNAT, RFC 6598)
    let cgnat = o[0] == 100 && (o[1] & 0xC0) == 64;

    !(v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_unspecified()
        || v4.is_multicast()
        || documentation
        || benchmark
        || cgnat)
}

fn is_public_ipv6(v6: Ipv6Addr) -> bool {
    // `::` and `::1` also have the IPv4-compatible form, so refuse them first.
    if v6.is_unspecified() || v6.is_loopback() {
        return false;
    }
    let s = v6.segments();
    let leading_zero = s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0;
    let v4_mapped = leading_zero && s[5] == 0xffff;
    let v4_compatible = leading_zero && s[5] == 0;
    let nat64 =
        s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0;
    if v4_mapped || v4_compatible || nat64 {
        let [first, second] = s[6].to_be_bytes();
        let [third, fourth] = s[7].to_be_bytes();
        return is_public_ipv4(Ipv4Addr::new(first, second, third, fourth));
    }
    let ula = (s[0] & 0xFE00) == 0xFC00;
    let link_local = (s[0] & 0xFFC0) == 0xFE80;
    let multicast = (s[0] & 0xFF00) == 0xFF00;
    let documentation = s[0] == 0x2001 && s[1] == 0x0db8;
    !(ula || link_local || multicast || documentation)
}

/// Examines `url` and gives the addresses of its host.
///
/// The scheme must be `http` or `https`. The URL must have no user name and no
/// password. A literal address must be public. A host name must resolve, and
/// each of its addresses must be public. The DNS lookup blocks the thread.
///
/// # Errors
///
/// The [`GuardError`] of the first check that fails.
pub fn check_url(url: &Url) -> Result<CheckedHost, GuardError> {
    match target_of(url)? {
        Target::Ip(host, addr) => checked(host, vec![addr], AddressRule::Public),
        Target::Domain(host, port) => {
            let addrs = (host.as_str(), port)
                .to_socket_addrs()
                .map_err(|error| GuardError::Dns(format!("{host}: {error}")))?
                .collect();
            checked(host, addrs, AddressRule::Public)
        }
    }
}

/// The host of a URL, after the checks that need no DNS lookup.
enum Target {
    Ip(String, SocketAddr),
    Domain(String, u16),
}

fn target_of(url: &Url) -> Result<Target, GuardError> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(GuardError::Scheme(scheme.to_owned())),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(GuardError::Credentials);
    }
    let port = url.port_or_known_default().ok_or(GuardError::NoHost)?;
    match url.host() {
        Some(Host::Ipv4(ip)) => Ok(Target::Ip(ip.to_string(), SocketAddr::new(ip.into(), port))),
        Some(Host::Ipv6(ip)) => Ok(Target::Ip(ip.to_string(), SocketAddr::new(ip.into(), port))),
        Some(Host::Domain(domain)) if !domain.is_empty() => {
            Ok(Target::Domain(domain.to_owned(), port))
        }
        _ => Err(GuardError::NoHost),
    }
}

/// The address rule of a check. Production code uses only `Public`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AddressRule {
    Public,
    #[cfg_attr(not(all(feature = "test-support", debug_assertions)), allow(dead_code))]
    PublicOrTestAddress(SocketAddr),
}

impl AddressRule {
    fn allows(self, addr: SocketAddr) -> bool {
        match self {
            Self::Public => is_public_ip(addr.ip()),
            Self::PublicOrTestAddress(test) => addr == test || is_public_ip(addr.ip()),
        }
    }
}

fn checked(
    host: String,
    addrs: Vec<SocketAddr>,
    rule: AddressRule,
) -> Result<CheckedHost, GuardError> {
    if addrs.is_empty() {
        return Err(GuardError::NoAddress(host));
    }
    if let Some(refused) = addrs.iter().find(|addr| !rule.allows(**addr)) {
        return Err(GuardError::Address(refused.ip()));
    }
    Ok(CheckedHost { host, addrs })
}

#[cfg(feature = "fetch")]
mod fetch;

#[cfg(feature = "fetch")]
pub use fetch::GuardedFetch;
