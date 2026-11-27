//! The read of one `podcast:remoteItem` (musicindex ADR 0092 R-STATION-1
//! item 3).

use std::fmt;

use serde_json::Value;
use stophammer_parser::types::IngestAlternateEnclosure;
use stophammer_parser::{
    FeedValues, IngestFeedData, IngestRemoteFeedRef, IngestTrackData, web_url_or_none,
};

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
///
/// The list does not contain a remote item with the medium `publisher`. Such
/// an item points to the feed of the publisher, not to a song. The parser
/// gives this medium to a `podcast:remoteItem` in `podcast:publisher` and to
/// a `podcast:remoteItem` with `medium="publisher"`.
#[must_use]
pub fn song_list(feed: &FeedValues) -> Vec<RemoteItemRef> {
    feed.remote_items
        .iter()
        .filter(|item| {
            !item
                .medium
                .as_deref()
                .is_some_and(|medium| medium.trim().eq_ignore_ascii_case("publisher"))
        })
        .map(RemoteItemRef::from)
        .collect()
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
    /// (musicindex ADR 0101 R-TAG-4 item 4).
    pub artist: Option<String>,
    /// The `itunes:author` of the channel.
    pub author: Option<String>,
    /// The enclosure URL. `None` when the item has no enclosure.
    pub enclosure_url: Option<String>,
    /// The enclosure MIME type.
    pub enclosure_type: Option<String>,
    /// The `podcast:alternateEnclosure` elements of the item, as the parser
    /// gives them (musicindex ADR 0095 R-MP3-2 item 3).
    pub alternate_enclosures: Vec<IngestAlternateEnclosure>,
    /// The image URL of the item, else of the channel (musicindex ADR 0096
    /// R-IMAGE-1). `None` when no source gives a good URL. [`image_url`]
    /// gives the selection.
    pub image_url: Option<String>,
    /// The value block of the item, else of the channel, in the value form of
    /// `TXXX:Podcast Value Routes` (musicindex ADR 0101 R-TAG-6 item 1).
    /// `None` when the feed has no Lightning block.
    pub value_routes: Option<Value>,
}

impl ResolvedItem {
    /// The URL of the MP3 file that the feed gives for the item, or `None`
    /// (musicindex ADR 0095 R-MP3-2 item 1). [`track_mp3_url`] gives the
    /// selection.
    #[must_use]
    pub fn mp3_url(&self) -> Option<&str> {
        mp3_url(
            self.enclosure_url.as_deref(),
            self.enclosure_type.as_deref(),
            &self.alternate_enclosures,
        )
    }
}

/// The URL of the MP3 file that the feed gives for `track`, or `None`
/// (musicindex ADR 0095 R-MP3-2 item 1).
///
/// The function selects the enclosure when its type is `audio/mpeg`. Else,
/// it selects the first `podcast:alternateEnclosure` with the type
/// `audio/mpeg`, in feed order. A type matches with any letter case, with
/// spaces around it and with parameters after a `;`. An enclosure with no
/// URL or an empty URL is not selected.
#[must_use]
pub fn track_mp3_url<R>(track: &IngestTrackData<R>) -> Option<&str> {
    mp3_url(
        track.enclosure_url.as_deref(),
        track.enclosure_type.as_deref(),
        &track.alternate_enclosures,
    )
}

fn mp3_url<'a>(
    url: Option<&'a str>,
    mime_type: Option<&str>,
    alternates: &'a [IngestAlternateEnclosure],
) -> Option<&'a str> {
    let usable = |url: &'a str| Some(url.trim()).filter(|url| !url.is_empty());
    if is_mp3(mime_type)
        && let Some(url) = url.and_then(usable)
    {
        return Some(url);
    }
    alternates
        .iter()
        .filter(|alternate| is_mp3(alternate.mime_type.as_deref()))
        .filter_map(|alternate| usable(&alternate.url).map(|url| (alternate.position, url)))
        .min_by_key(|(position, _)| *position)
        .map(|(_, url)| url)
}

fn is_mp3(mime_type: Option<&str>) -> bool {
    mime_type.is_some_and(|mime_type| {
        mime_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("audio/mpeg")
    })
}

/// The maximum length of an image URL in characters (musicindex ADR 0096
/// R-IMAGE-1 item 3).
pub const MAX_IMAGE_URL_CHARS: usize = 2048;

/// The image URL of `track` in `feed` (musicindex ADR 0096 R-IMAGE-1), or
/// `None`. No image is not an error.
///
/// The function takes the first good URL of these sources:
///
/// 1. The `itunes:image` of the item.
/// 2. A `podcast:image` of the item with the purpose `artwork` or with no
///    purpose, in feed order. The purpose is a list of words, and one word
///    must be `artwork`.
/// 3. The `podcast:images` `srcset` entry of the item with the largest
///    width. Of two entries with that width, the first in the `srcset`. An
///    entry with no width is not a candidate.
/// 4. The `itunes:image` of the channel.
/// 5. The RSS `image` `url` of the channel.
///
/// A good URL has the scheme `http` or `https` and a maximum of
/// [`MAX_IMAGE_URL_CHARS`] characters (R-IMAGE-1 item 3). The function does
/// not shorten a URL. A URL that is not good goes to the next candidate.
/// When the largest `srcset` entry is not good, the function goes to
/// source 4, not to a smaller entry.
#[must_use]
pub fn image_url<R, S>(feed: &IngestFeedData<S>, track: &IngestTrackData<R>) -> Option<String> {
    // max_by_key gives the last of equal entries, so the reversed
    // iterator gives the first.
    let largest = track
        .podcast_images_srcset
        .iter()
        .rev()
        .filter_map(|entry| entry.width.map(|width| (width, entry.url.as_str())))
        .max_by_key(|(width, _)| *width)
        .map(|(_, url)| url);
    track
        .itunes_image_url
        .as_deref()
        .into_iter()
        .chain(
            track
                .podcast_images
                .iter()
                .filter(|image| is_artwork(image.purpose.as_deref()))
                .map(|image| image.href.as_str()),
        )
        .chain(largest)
        .chain(feed.itunes_image_url.as_deref())
        .chain(feed.rss_image_url.as_deref())
        .find_map(good_image_url)
}

fn is_artwork(purpose: Option<&str>) -> bool {
    purpose.is_none_or(|purpose| {
        purpose
            .split_whitespace()
            .any(|word| word.eq_ignore_ascii_case("artwork"))
    })
}

fn good_image_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.chars().count() > MAX_IMAGE_URL_CHARS {
        return None;
    }
    web_url_or_none(Some(url))
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
        alternate_enclosures: track.alternate_enclosures.clone(),
        image_url: image_url(feed, track),
        value_routes: value_routes(&track.value_blocks, &track.payment_routes)
            .or_else(|| value_routes(&feed.feed_value_blocks, &feed.feed_payment_routes)),
    })
}
