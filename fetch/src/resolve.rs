//! The read of one `podcast:remoteItem` (musicindex ADR 0092 R-STATION-1
//! item 3).

use std::fmt;

use serde_json::Value;
use stophammer_parser::{FeedValues, IngestRemoteFeedRef};

use crate::reader::{FeedDocument, FeedError, FeedReader, FetchDocument};
use crate::routes::value_routes;

/// One `podcast:remoteItem`: the song of a `musicL` feed or of a split.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteItemRef {
    /// The attribute `feedGuid`.
    pub feed_guid: String,
    /// The attribute `feedUrl`. `None` when the attribute is missing.
    pub feed_url: Option<String>,
    /// The attribute `itemGuid`. `None` when the attribute is missing.
    pub item_guid: Option<String>,
    /// The attribute `title`. `None` when the attribute is missing.
    pub title: Option<String>,
}

impl From<&IngestRemoteFeedRef> for RemoteItemRef {
    fn from(item: &IngestRemoteFeedRef) -> Self {
        Self {
            feed_guid: item.remote_feed_guid.clone(),
            feed_url: item
                .remote_feed_url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .map(str::to_owned),
            item_guid: item.item_guid.clone(),
            title: item.item_title.clone(),
        }
    }
}

/// The song list of a `musicL` feed: each channel `podcast:remoteItem`, in
/// feed order (musicindex ADR 0092 R-STATION-3 item 1).
#[must_use]
pub fn song_list(feed: &FeedValues) -> Vec<RemoteItemRef> {
    feed.remote_items.iter().map(RemoteItemRef::from).collect()
}

/// The data of one remote item after the read of its feed.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedItem {
    /// The feed GUID of the remote item.
    pub feed_guid: String,
    /// The item GUID of the remote item.
    pub item_guid: String,
    /// The feed URL that the reader read.
    pub feed_url: String,
    /// The item title.
    pub title: Option<String>,
    /// The `itunes:author` of the item. The channel author never replaces it
    /// (musicindex ADR 0093 R-TAG-4 item 4).
    pub artist: Option<String>,
    /// The `itunes:author` of the channel.
    pub author: Option<String>,
    /// The enclosure URL. `None` when the item has no enclosure.
    pub enclosure_url: Option<String>,
    /// The enclosure MIME type.
    pub enclosure_type: Option<String>,
    /// The value block of the item, else of the channel, in the value form of
    /// `TXXX:Podcast Value Routes` (musicindex ADR 0093 R-TAG-6 item 1).
    /// `None` when the feed has no Lightning block.
    pub value_routes: Option<Value>,
}

/// The error of [`FeedReader::resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// The remote item has no `itemGuid`.
    NoItemGuid {
        /// The feed GUID.
        feed_guid: String,
    },
    /// The feed URL lookup or the feed read failed.
    Feed(FeedError),
    /// The feed has no item with the item GUID.
    NoItem {
        /// The feed URL.
        feed_url: String,
        /// The item GUID.
        item_guid: String,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoItemGuid { feed_guid } => {
                write!(f, "the remote item of feed {feed_guid} has no itemGuid")
            }
            Self::Feed(error) => error.fmt(f),
            Self::NoItem {
                feed_url,
                item_guid,
            } => write!(f, "{feed_url} has no item with the GUID {item_guid}"),
        }
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Feed(error) => Some(error),
            _ => None,
        }
    }
}

impl From<FeedError> for ResolveError {
    fn from(error: FeedError) -> Self {
        Self::Feed(error)
    }
}

impl<F: FetchDocument> FeedReader<F> {
    /// Reads the feed of `item` and gives the data of its item. When `item`
    /// has no feed URL, the reader gets it from MusicIndex by the feed GUID
    /// (musicindex ADR 0092 R-STATION-1 item 3).
    ///
    /// # Errors
    ///
    /// Gives [`ResolveError`] when the item has no item GUID, when the feed
    /// cannot be read, or when the feed has no item with the item GUID.
    pub fn resolve(&mut self, item: &RemoteItemRef) -> Result<ResolvedItem, ResolveError> {
        let item_guid = item
            .item_guid
            .as_deref()
            .map(str::trim)
            .filter(|guid| !guid.is_empty())
            .ok_or_else(|| ResolveError::NoItemGuid {
                feed_guid: item.feed_guid.clone(),
            })?;
        let feed_url = match &item.feed_url {
            Some(url) => url.clone(),
            None => self.feed_url_for_guid(&item.feed_guid)?,
        };
        let document = self.read(&feed_url)?;
        resolved_item(&document, &item.feed_guid, item_guid)
    }
}

fn resolved_item(
    document: &FeedDocument,
    feed_guid: &str,
    item_guid: &str,
) -> Result<ResolvedItem, ResolveError> {
    let feed = &document.values;
    let track = feed
        .tracks
        .iter()
        .find(|track| track.track_guid.as_deref().map(str::trim) == Some(item_guid))
        .ok_or_else(|| ResolveError::NoItem {
            feed_url: document.url.clone(),
            item_guid: item_guid.to_owned(),
        })?;
    Ok(ResolvedItem {
        feed_guid: feed_guid.to_owned(),
        item_guid: item_guid.to_owned(),
        feed_url: document.url.clone(),
        title: track.title.clone(),
        artist: track.author_name.clone(),
        author: feed.author_name.clone(),
        enclosure_url: track.enclosure_url.clone(),
        enclosure_type: track.enclosure_type.clone(),
        value_routes: value_routes(&track.payment_routes)
            .or_else(|| value_routes(&feed.feed_payment_routes)),
    })
}
