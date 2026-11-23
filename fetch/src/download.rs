//! The download of one enclosure (musicindex ADR 0092 R-STATION-1 item 4).

use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use sha2::{Digest, Sha256};

const MAX_REDIRECTS: usize = 10;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The result of a good download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    /// The SHA-256 hash of the file, as 64 lowercase hexadecimal characters.
    pub sha256: String,
    /// The size of the file in bytes.
    pub bytes: u64,
}

/// The error of a download. The path has no new file.
#[derive(Debug)]
pub enum DownloadError {
    /// The URL is not an HTTP or HTTPS URL.
    BadUrl(String),
    /// The answer had an HTTP error status.
    Status {
        /// The URL.
        url: String,
        /// The HTTP status code.
        status: u16,
    },
    /// The request or the read of the body failed.
    Transport(reqwest::Error),
    /// The write of the temporary file or its move to the path failed.
    Io(io::Error),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadUrl(url) => write!(f, "{url} is not an HTTP or HTTPS URL"),
            Self::Status { url, status } => write!(f, "{url} gave HTTP {status}"),
            Self::Transport(error) => write!(f, "download failed: {error}"),
            Self::Io(error) => write!(f, "download write failed: {error}"),
        }
    }
}

impl std::error::Error for DownloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for DownloadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Downloads `url` to `path` with a client of this crate: a connect timeout
/// of 10 seconds, a read timeout of 30 seconds, a maximum of 10 HTTP or
/// HTTPS redirects, and no decode of a compressed body. [`download_enclosure_with`] gives the steps.
///
/// # Errors
///
/// Gives [`DownloadError`] when the client cannot be made or the download
/// fails.
pub fn download_enclosure(
    url: &str,
    path: &Path,
    user_agent: &str,
) -> Result<Download, DownloadError> {
    let client = Client::builder()
        .user_agent(user_agent)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(READ_TIMEOUT)
        // The file keeps the bytes of the answer, so the hash is the hash of
        // those bytes. The client asks for no compression and does not
        // decode a body.
        .no_brotli()
        .no_deflate()
        .no_gzip()
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if matches!(attempt.url().scheme(), "http" | "https") {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(DownloadError::Transport)?;
    download_enclosure_with(&client, url, path)
}

/// Downloads `url` to `path` with `client` (musicindex ADR 0092 R-STATION-1
/// item 4).
///
/// The function makes the folder of `path`, writes the body to a temporary
/// file in that folder and computes the SHA-256 hash of the bytes. After the
/// full body is written, it moves the temporary file to `path`, and replaces
/// a file at `path`. When a step fails, it removes the temporary file, so
/// the download leaves no file at `path`. A file that was at `path` before
/// the call stays.
///
/// # Errors
///
/// Gives [`DownloadError`] for a URL that is not HTTP or HTTPS, an HTTP error
/// status, a failed request or read, or a failed write.
pub fn download_enclosure_with(
    client: &Client,
    url: &str,
    path: &Path,
) -> Result<Download, DownloadError> {
    match reqwest::Url::parse(url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => {}
        _ => return Err(DownloadError::BadUrl(url.to_owned())),
    }
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    fs::create_dir_all(folder)?;

    let mut response = client.get(url).send().map_err(DownloadError::Transport)?;
    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::Status {
            url: url.to_owned(),
            status: status.as_u16(),
        });
    }

    // The temporary file is removed when it is dropped before `persist`.
    let mut temporary = tempfile::Builder::new()
        .prefix(".download-")
        .tempfile_in(folder)?;
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = match response.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(DownloadError::Io(error)),
        };
        hasher.update(&buffer[..count]);
        temporary.write_all(&buffer[..count])?;
        bytes += count as u64;
    }
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| DownloadError::Io(error.error))?;

    let sha256 = hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        });
    Ok(Download { sha256, bytes })
}
