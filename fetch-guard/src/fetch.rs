//! The guarded fetch of the feature `fetch`.

use std::time::{Duration, Instant};

use url::Url;

use crate::{AddressRule, CheckedHost, GuardError, Target, checked, target_of};

/// A fetch that examines each hop of a redirect chain and pins the connection
/// of each hop to the addresses that it examined.
///
/// Production code uses [`GuardedFetch::new`] and the setters for the limits.
/// The address rule is always the public rule of [`crate::is_public_ip`],
/// except with `accept_address_for_tests` of the feature `test-support`.
#[derive(Clone, Debug)]
pub struct GuardedFetch {
    user_agent: String,
    max_redirects: usize,
    hop_timeout: Duration,
    chain_timeout: Duration,
    max_body_bytes: usize,
    rule: AddressRule,
}

impl GuardedFetch {
    /// The default number of redirects.
    pub const DEFAULT_MAX_REDIRECTS: usize = 3;
    /// The default time limit of each hop.
    pub const DEFAULT_HOP_TIMEOUT: Duration = Duration::from_secs(10);
    /// The default time limit of the full redirect chain.
    pub const DEFAULT_CHAIN_TIMEOUT: Duration = Duration::from_secs(30);
    /// The default limit of the body.
    pub const DEFAULT_MAX_BODY_BYTES: usize = 5 * 1024 * 1024;

    /// A fetch with the default limits and the public address rule.
    #[must_use]
    pub fn new(user_agent: impl Into<String>) -> Self {
        Self {
            user_agent: user_agent.into(),
            max_redirects: Self::DEFAULT_MAX_REDIRECTS,
            hop_timeout: Self::DEFAULT_HOP_TIMEOUT,
            chain_timeout: Self::DEFAULT_CHAIN_TIMEOUT,
            max_body_bytes: Self::DEFAULT_MAX_BODY_BYTES,
            rule: AddressRule::Public,
        }
    }

    /// Sets the maximum number of redirects. With 0, a redirect is an error.
    #[must_use]
    pub fn with_max_redirects(mut self, max_redirects: usize) -> Self {
        self.max_redirects = max_redirects;
        self
    }

    /// Sets the time limit of each hop and of the full chain. The limit of a
    /// hop includes the DNS lookup of the hop.
    #[must_use]
    pub fn with_timeouts(mut self, hop: Duration, chain: Duration) -> Self {
        self.hop_timeout = hop;
        self.chain_timeout = chain;
        self
    }

    /// Sets the maximum size of the body in bytes.
    #[must_use]
    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Self {
        self.max_body_bytes = max_body_bytes;
        self
    }

    /// Also accepts the one socket address `addr`, for a test server on the
    /// loopback address. The address and the port must agree.
    ///
    /// Only a test uses this. Production code uses the default rule. Do not
    /// set this from configuration or from an environment variable. The
    /// method exists only with the feature `test-support`, which only a
    /// dev-dependency turns on, and only in a build with debug assertions. A
    /// release build with `--all-features` does not have it.
    #[cfg(all(feature = "test-support", debug_assertions))]
    #[must_use]
    pub fn accept_address_for_tests(mut self, addr: std::net::SocketAddr) -> Self {
        self.rule = AddressRule::PublicOrTestAddress(addr);
        self
    }

    /// Fetches `url` and gives the bytes of the body.
    ///
    /// The fetch examines the URL of each hop with the checks of
    /// [`crate::check_url`], and connects only to the addresses that it
    /// examined. It follows a relative or an absolute `Location`.
    ///
    /// # Errors
    ///
    /// The [`GuardError`] of the first check or request that fails.
    pub async fn fetch(&self, url: &Url) -> Result<Vec<u8>, GuardError> {
        let deadline = Instant::now().checked_add(self.chain_timeout);
        let mut current = url.clone();
        for hop in 0..=self.max_redirects {
            let hop_limit = deadline.map_or(self.hop_timeout, |deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(self.hop_timeout)
            });
            if hop_limit.is_zero() {
                return Err(GuardError::Timeout);
            }
            let hop_end = Instant::now().checked_add(hop_limit);
            let pinned = tokio::time::timeout(hop_limit, self.check(&current))
                .await
                .map_err(|_| GuardError::Timeout)??;
            let client = self.client(&pinned)?;
            let request_limit = hop_end.map_or(hop_limit, |end| {
                end.saturating_duration_since(Instant::now())
            });
            if request_limit.is_zero() {
                return Err(GuardError::Timeout);
            }
            let mut response = client
                .get(current.as_str())
                .timeout(request_limit)
                .send()
                .await
                .map_err(request_error)?;
            let status = response.status();
            if !status.is_redirection() {
                if !status.is_success() {
                    return Err(GuardError::Status(status.as_u16()));
                }
                if response
                    .content_length()
                    .is_some_and(|length| length > self.max_body_bytes as u64)
                {
                    return Err(GuardError::TooLarge(self.max_body_bytes));
                }
                let mut body = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(request_error)? {
                    body.extend_from_slice(&chunk);
                    if body.len() > self.max_body_bytes {
                        return Err(GuardError::TooLarge(self.max_body_bytes));
                    }
                }
                return Ok(body);
            }
            if hop == self.max_redirects {
                break;
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .ok_or_else(|| GuardError::BadLocation("no Location header".to_owned()))?
                .to_str()
                .map_err(|error| GuardError::BadLocation(error.to_string()))?;
            current = current
                .join(location)
                .map_err(|error| GuardError::BadLocation(error.to_string()))?;
        }
        Err(GuardError::TooManyRedirects(self.max_redirects))
    }

    /// The checks of [`crate::check_url`] with the rule of this fetch, and an
    /// async DNS lookup.
    async fn check(&self, url: &Url) -> Result<CheckedHost, GuardError> {
        match target_of(url)? {
            Target::Ip(host, addr) => checked(host, vec![addr], self.rule),
            Target::Domain(host, port) => {
                let addrs = tokio::net::lookup_host((host.as_str(), port))
                    .await
                    .map_err(|error| GuardError::Dns(format!("{host}: {error}")))?
                    .collect();
                checked(host, addrs, self.rule)
            }
        }
    }

    /// A client for one hop: no redirect, no proxy, and the DNS of the host
    /// pinned to the checked addresses. A proxy does its own DNS lookup, so the
    /// client ignores a proxy of the environment.
    fn client(&self, pinned: &CheckedHost) -> Result<reqwest::Client, GuardError> {
        reqwest::Client::builder()
            .user_agent(self.user_agent.as_str())
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&pinned.host, &pinned.addrs)
            .build()
            .map_err(request_error)
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "map_err passes the error by value"
)]
fn request_error(error: reqwest::Error) -> GuardError {
    if error.is_timeout() {
        GuardError::Timeout
    } else {
        GuardError::Request(error.to_string())
    }
}
