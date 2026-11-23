//! The feed read with the host limits (musicindex ADR 0092 R-STATION-1 item 2).

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::blocking::Client;
use stophammer_parser::FeedValues;

/// The minimum time between two requests to one host. HitIt used this value
/// before the crate (`RSS_HOST_INTERVAL`).
pub const RSS_HOST_INTERVAL: Duration = Duration::from_secs(2);

/// The base URL of the MusicIndex API.
pub const DEFAULT_MUSICINDEX_BASE_URL: &str = "https://api.musicindex.org";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

/// The settings of a [`FeedReader`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderConfig {
    /// The minimum time between two requests to one host.
    pub min_interval: Duration,
    /// The base URL of the MusicIndex API, with no `/` at the end.
    pub musicindex_base_url: String,
    /// The `User-Agent` of each request of [`HttpFetch`].
    pub user_agent: String,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            min_interval: RSS_HOST_INTERVAL,
            musicindex_base_url: DEFAULT_MUSICINDEX_BASE_URL.to_owned(),
            user_agent: concat!("musicindex-fetch/", env!("CARGO_PKG_VERSION")).to_owned(),
        }
    }
}

/// HTTP 429 (Too Many Requests) from a host. The reader sends no more
/// requests to that host in the run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostThrottled {
    /// The lowercase host of the URL.
    pub host: String,
    /// The parsed `Retry-After` value, or `None` when the response has none.
    pub retry_after: Option<Duration>,
}

impl fmt::Display for HostThrottled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} sent HTTP 429 (Too Many Requests)", self.host)
    }
}

impl std::error::Error for HostThrottled {}

/// The error of one feed read or of one MusicIndex lookup. The reader keeps
/// the error of each URL, so the type is `Clone`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedError {
    /// The URL is not an HTTP or HTTPS URL with a host.
    BadUrl {
        /// The URL.
        url: String,
    },
    /// The host answered HTTP 429 to this request.
    Throttled(HostThrottled),
    /// An earlier request of the run got HTTP 429 from this host, so the
    /// reader did not send this request.
    HostStopped {
        /// The URL that the reader did not read.
        url: String,
        /// The lowercase host.
        host: String,
    },
    /// The answer had an HTTP error status.
    Status {
        /// The URL.
        url: String,
        /// The HTTP status code.
        status: u16,
    },
    /// The request or the read of the answer failed.
    Transport {
        /// The URL.
        url: String,
        /// The cause.
        message: String,
    },
    /// The answer is not a feed that the parser can read.
    Parse {
        /// The URL.
        url: String,
        /// The cause.
        message: String,
    },
    /// MusicIndex gave no feed URL for the feed GUID.
    NoFeedUrl {
        /// The feed GUID.
        feed_guid: String,
    },
}

impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadUrl { url } => write!(f, "{url} is not an HTTP or HTTPS URL"),
            Self::Throttled(throttled) => throttled.fmt(f),
            Self::HostStopped { url, host } => write!(
                f,
                "{host} sent HTTP 429 earlier in this run, so {url} was not read"
            ),
            Self::Status { url, status } => write!(f, "{url} gave HTTP {status}"),
            Self::Transport { url, message } => write!(f, "GET {url}: {message}"),
            Self::Parse { url, message } => write!(f, "{url} is not a readable feed: {message}"),
            Self::NoFeedUrl { feed_guid } => {
                write!(
                    f,
                    "MusicIndex gives no feed URL for the feed GUID {feed_guid}"
                )
            }
        }
    }
}

impl std::error::Error for FeedError {}

/// The lowercase host of an HTTP or HTTPS URL.
#[must_use]
pub fn feed_host(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.host_str()
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
}

/// Reads a `Retry-After` text: a number of seconds, or an HTTP date. A date
/// in the past gives a zero wait.
#[must_use]
pub fn parse_retry_after(text: &str) -> Option<Duration> {
    let text = text.trim();
    if let Ok(seconds) = text.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = chrono::DateTime::parse_from_rfc2822(text).ok()?;
    let wait = at.with_timezone(&chrono::Utc) - chrono::Utc::now();
    Some(wait.to_std().unwrap_or_default())
}

/// One GET request for the [`FeedReader`]. A test or an app can give its own
/// implementation. A closure `FnMut(&str) -> Result<Vec<u8>, FeedError>` is
/// also an implementation.
pub trait FetchDocument {
    /// The body of the answer to a GET request for `url`.
    ///
    /// # Errors
    ///
    /// Gives [`FeedError::Throttled`] for HTTP 429. The reader then stops the
    /// host. Each other error stops only this URL.
    fn fetch(&mut self, url: &str) -> Result<Vec<u8>, FeedError>;
}

impl<F> FetchDocument for F
where
    F: FnMut(&str) -> Result<Vec<u8>, FeedError>,
{
    fn fetch(&mut self, url: &str) -> Result<Vec<u8>, FeedError> {
        self(url)
    }
}

/// The HTTP implementation of [`FetchDocument`], with `reqwest`.
#[derive(Clone, Debug)]
pub struct HttpFetch {
    client: Client,
}

impl HttpFetch {
    /// Makes the client with the `User-Agent` of `config`, a connect timeout
    /// of 10 seconds and a timeout of 30 seconds.
    ///
    /// # Errors
    ///
    /// Gives an error when `reqwest` cannot make the client.
    pub fn new(config: &ReaderConfig) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .user_agent(config.user_agent.clone())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(OPERATION_TIMEOUT)
            .build()?;
        Ok(Self { client })
    }
}

impl FetchDocument for HttpFetch {
    fn fetch(&mut self, url: &str) -> Result<Vec<u8>, FeedError> {
        let transport = |error: reqwest::Error| FeedError::Transport {
            url: url.to_owned(),
            message: error.to_string(),
        };
        let response = self.client.get(url).send().map_err(transport)?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after);
            // After a redirect, the host that answered 429 is the host of
            // the last URL.
            let host = feed_host(response.url().as_str())
                .or_else(|| feed_host(url))
                .unwrap_or_else(|| url.to_owned());
            return Err(FeedError::Throttled(HostThrottled { host, retry_after }));
        }
        if !status.is_success() {
            return Err(FeedError::Status {
                url: url.to_owned(),
                status: status.as_u16(),
            });
        }
        Ok(response.bytes().map_err(transport)?.to_vec())
    }
}

/// One feed that the reader read: the URL, the body and the values of the
/// parser.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedDocument {
    /// The URL of the request.
    pub url: String,
    /// The body of the answer. An app can store it.
    pub body: Vec<u8>,
    /// The values of `stophammer-parser`, with no required-field check
    /// (musicindex ADR 0093 R-FEED-2).
    pub values: FeedValues,
}

impl FeedDocument {
    /// Parses `body` with `stophammer-parser`. `url` is the URL or the path
    /// of the feed, for the error text.
    ///
    /// # Errors
    ///
    /// Gives [`FeedError::Parse`] when the body is not UTF-8 or the parser
    /// cannot read it.
    pub fn parse(url: &str, body: Vec<u8>) -> Result<Self, FeedError> {
        let parse_error = |message: String| FeedError::Parse {
            url: url.to_owned(),
            message,
        };
        let text = std::str::from_utf8(&body).map_err(|error| parse_error(error.to_string()))?;
        let values = stophammer_parser::profile::stophammer()
            .read(text)
            .map_err(|error| parse_error(error.to_string()))?;
        Ok(Self {
            url: url.to_owned(),
            body,
            values,
        })
    }
}

/// Reads feeds for one run. It reads each URL one time, keeps
/// [`ReaderConfig::min_interval`] between two requests to one host, and sends
/// no more requests to a host that answered HTTP 429 (musicindex ADR 0092
/// R-STATION-1 item 2).
pub struct FeedReader<F = HttpFetch> {
    fetch: F,
    config: ReaderConfig,
    documents: HashMap<String, Result<Arc<FeedDocument>, FeedError>>,
    feed_urls: HashMap<String, Result<String, FeedError>>,
    last_request: HashMap<String, Instant>,
    stopped_hosts: BTreeSet<String>,
}

impl FeedReader<HttpFetch> {
    /// A reader with [`HttpFetch`].
    ///
    /// # Errors
    ///
    /// Gives an error when `reqwest` cannot make the client.
    pub fn new(config: ReaderConfig) -> Result<Self, reqwest::Error> {
        let fetch = HttpFetch::new(&config)?;
        Ok(Self::with_fetch(fetch, config))
    }
}

impl<F: FetchDocument> FeedReader<F> {
    /// A reader with the request function `fetch`.
    pub fn with_fetch(fetch: F, config: ReaderConfig) -> Self {
        Self {
            fetch,
            config,
            documents: HashMap::new(),
            feed_urls: HashMap::new(),
            last_request: HashMap::new(),
            stopped_hosts: BTreeSet::new(),
        }
    }

    /// The hosts that answered HTTP 429 in this run.
    pub fn stopped_hosts(&self) -> impl Iterator<Item = &str> {
        self.stopped_hosts.iter().map(String::as_str)
    }

    /// Reads and parses the feed at `url`. A second call for the same URL
    /// gives the first result and sends no request.
    ///
    /// # Errors
    ///
    /// Gives the [`FeedError`] of the request or of the parse.
    pub fn read(&mut self, url: &str) -> Result<Arc<FeedDocument>, FeedError> {
        if let Some(result) = self.documents.get(url) {
            return result.clone();
        }
        let result = self
            .get(url)
            .and_then(|body| FeedDocument::parse(url, body))
            .map(Arc::new);
        self.documents.insert(url.to_owned(), result.clone());
        result
    }

    /// The feed URL that MusicIndex gives for `feed_guid`
    /// (`GET /v1/feeds/{guid}`, field `data.feed_url`). The reader asks one
    /// time for each GUID in a run.
    ///
    /// # Errors
    ///
    /// Gives the [`FeedError`] of the request, [`FeedError::Parse`] when the
    /// answer is not JSON, and [`FeedError::NoFeedUrl`] when the answer has
    /// no feed URL.
    pub fn feed_url_for_guid(&mut self, feed_guid: &str) -> Result<String, FeedError> {
        if let Some(result) = self.feed_urls.get(feed_guid) {
            return result.clone();
        }
        let result = self.lookup_feed_url(feed_guid);
        self.feed_urls.insert(feed_guid.to_owned(), result.clone());
        result
    }

    fn lookup_feed_url(&mut self, feed_guid: &str) -> Result<String, FeedError> {
        let base = self.config.musicindex_base_url.trim_end_matches('/');
        let mut url = reqwest::Url::parse(base).map_err(|_| FeedError::BadUrl {
            url: base.to_owned(),
        })?;
        url.path_segments_mut()
            .map_err(|()| FeedError::BadUrl {
                url: base.to_owned(),
            })?
            .extend(["v1", "feeds", feed_guid]);
        let url = url.to_string();
        let body = self.get(&url)?;
        let answer: serde_json::Value =
            serde_json::from_slice(&body).map_err(|error| FeedError::Parse {
                url: url.clone(),
                message: error.to_string(),
            })?;
        answer
            .pointer("/data/feed_url")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|feed_url| !feed_url.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| FeedError::NoFeedUrl {
                feed_guid: feed_guid.to_owned(),
            })
    }

    /// One request with the host limits.
    fn get(&mut self, url: &str) -> Result<Vec<u8>, FeedError> {
        let host = feed_host(url).ok_or_else(|| FeedError::BadUrl {
            url: url.to_owned(),
        })?;
        if self.stopped_hosts.contains(&host) {
            return Err(FeedError::HostStopped {
                url: url.to_owned(),
                host,
            });
        }
        if let Some(wait) = self
            .last_request
            .get(&host)
            .and_then(|last| self.config.min_interval.checked_sub(last.elapsed()))
        {
            thread::sleep(wait);
        }
        self.last_request.insert(host.clone(), Instant::now());
        let result = self.fetch.fetch(url);
        if let Err(FeedError::Throttled(throttled)) = &result {
            self.stopped_hosts.insert(throttled.host.clone());
            self.stopped_hosts.insert(host);
        }
        result
    }
}
