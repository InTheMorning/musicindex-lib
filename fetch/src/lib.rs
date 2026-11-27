//! Reads the remote items of a feed and downloads an enclosure for the suite
//! (musicindex ADR 0092 R-STATION-1 and R-STATION-2).
//!
//! HitIt and musicindex-live-publisher use this crate. An app keeps no second
//! copy of the feed read or of the download (R-STATION-1 item 5).
//!
//! - [`FeedReader`] reads each feed URL one time in a run with
//!   `stophammer-parser` (musicindex ADR 0101 R-FEED-1). It keeps a minimum
//!   time between two requests to one host, and it stops a host that answers
//!   HTTP 429.
//! - [`FeedReader::resolve`] gives the feed GUID, the item GUID, the
//!   enclosure URL and the value block of one `podcast:remoteItem`. When the
//!   item has no feed URL, the reader gets it from MusicIndex by the feed
//!   GUID.
//! - [`ResolvedItem::mp3_url`] and [`track_mp3_url`] select the MP3 file
//!   that the feed gives for an item (musicindex ADR 0095 R-MP3-2 item 1).
//! - [`FeedReader::episode_splits`] changes the `podcast:valueTimeSplit`
//!   elements of an episode into [`musicindex_tags::Split`] values, with the
//!   routes of musicindex ADR 0101 R-TAG-6.
//! - [`download_enclosure`] writes one URL to one path through a temporary
//!   file, and gives the SHA-256 hash.
//!
//! `musicindex-tags` has no dependency on `stophammer-parser` (R-STATION-2
//! item 2). This crate holds the change from the parser types to the tag
//! types.

mod download;
mod reader;
mod resolve;
mod routes;
mod splits;

pub use download::{Download, DownloadError, download_enclosure, download_enclosure_with};
pub use reader::{
    DEFAULT_MUSICINDEX_BASE_URL, FeedDocument, FeedError, FeedReader, FetchDocument, HostThrottled,
    HttpFetch, RSS_HOST_INTERVAL, ReaderConfig, feed_host, parse_retry_after,
};
pub use resolve::{
    MAX_IMAGE_URL_CHARS, RemoteItemRef, ResolveError, ResolvedItem, image_url, song_list,
    track_mp3_url,
};
pub use routes::value_routes;
pub use splits::{BadSplit, BadSplitCause, EpisodeSplits};
pub use stophammer_parser;
