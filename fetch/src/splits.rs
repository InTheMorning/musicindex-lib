//! The chapter songs of an episode (musicindex ADR 0092 R-STATION-2).

use std::fmt;

use musicindex_tags::{Split, SplitTarget};
use stophammer_parser::{IngestTrackData, IngestValueTimeSplit, ValueTimeSplitForm};

use crate::reader::{FeedReader, FetchDocument};
use crate::resolve::{RemoteItemRef, ResolveError};
use crate::routes::recipients;

/// Why a `podcast:valueTimeSplit` gives no [`Split`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BadSplitCause {
    /// `startTime` is not a finite number of 0 or more.
    BadStartTime,
    /// The split has no `duration`, or it is not a finite number of 0 or
    /// more.
    BadDuration,
    /// `remotePercentage` is not a number from 0 to 100.
    BadRemotePercentage,
    /// A remote split has no `feedGuid`.
    NoFeedGuid,
}

/// A `podcast:valueTimeSplit` that gives no [`Split`]. The episode then gets
/// no chapter songs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadSplit {
    /// The position of the split in the item, from 0.
    pub index: usize,
    /// The cause.
    pub cause: BadSplitCause,
}

impl fmt::Display for BadSplit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cause = match self.cause {
            BadSplitCause::BadStartTime => "startTime is not a number of 0 or more",
            BadSplitCause::BadDuration => "duration is missing or not a number of 0 or more",
            BadSplitCause::BadRemotePercentage => "remotePercentage is not a number from 0 to 100",
            BadSplitCause::NoFeedGuid => "the remote item has no feedGuid",
        };
        write!(f, "valueTimeSplit {}: {cause}", self.index)
    }
}

impl std::error::Error for BadSplit {}

/// The chapter songs of one episode.
#[derive(Clone, Debug, PartialEq)]
pub struct EpisodeSplits {
    /// One [`Split`] for each `podcast:valueTimeSplit`, in feed order.
    pub splits: Vec<Split>,
    /// The remote splits whose song the reader could not read, with the
    /// position of the split. Such a split has no title and no routes
    /// (musicindex ADR 0093 R-TAG-6 item 2).
    pub unresolved: Vec<(usize, ResolveError)>,
}

impl<F: FetchDocument> FeedReader<F> {
    /// Changes the `podcast:valueTimeSplit` elements of `episode` into
    /// [`Split`] values for `musicindex_tags::write_episode_tags`
    /// (musicindex ADR 0092 R-STATION-2 item 1).
    ///
    /// For a remote split, the reader reads the song feed: the `feedUrl` of
    /// the split, else the feed URL from MusicIndex. The split gets the
    /// song title, the artist, the `itunes:author` of the song feed and the
    /// routes of R-TAG-6. When the read fails, these are `None`, and
    /// [`EpisodeSplits::unresolved`] gives the cause.
    ///
    /// # Errors
    ///
    /// Gives [`BadSplit`] for the first split with a bad time, a bad
    /// `remotePercentage`, or a remote split with no `feedGuid`.
    pub fn episode_splits<R>(
        &mut self,
        episode: &IngestTrackData<R>,
    ) -> Result<EpisodeSplits, BadSplit> {
        let mut splits = Vec::with_capacity(episode.value_time_splits.len());
        let mut unresolved = Vec::new();
        for (index, split) in episode.value_time_splits.iter().enumerate() {
            let bad = |cause| BadSplit { index, cause };
            let start_time = Some(split.start_time_secs)
                .filter(|time| time.is_finite() && *time >= 0.0)
                .ok_or(bad(BadSplitCause::BadStartTime))?;
            let duration = split
                .duration_secs
                .filter(|time| time.is_finite() && *time >= 0.0)
                .ok_or(bad(BadSplitCause::BadDuration))?;
            let remote_percentage =
                remote_percentage(split).map_err(|()| bad(BadSplitCause::BadRemotePercentage))?;
            let mut chapter = Split {
                start_time,
                duration,
                remote_percentage,
                title: None,
                artist: None,
                author: None,
                target: SplitTarget::Local {
                    value_routes: recipients(split.recipients.iter()),
                },
            };
            if split.form == ValueTimeSplitForm::Remote {
                let feed_guid = split
                    .remote_feed_guid
                    .as_deref()
                    .map(str::trim)
                    .filter(|guid| !guid.is_empty())
                    .ok_or(bad(BadSplitCause::NoFeedGuid))?;
                let item_guid = split
                    .remote_item_guid
                    .as_deref()
                    .map(str::trim)
                    .filter(|guid| !guid.is_empty())
                    .map(str::to_owned);
                // A split with a `feedUrl` reads that feed. A split with no
                // `feedUrl` gets the feed URL from MusicIndex.
                let item = RemoteItemRef {
                    feed_guid: feed_guid.to_owned(),
                    feed_url: split
                        .remote_feed_url
                        .as_deref()
                        .map(str::trim)
                        .filter(|url| !url.is_empty())
                        .map(str::to_owned),
                    item_guid: item_guid.clone(),
                    title: None,
                };
                let mut value_routes = None;
                match self.resolve(&item) {
                    Ok(song) => {
                        chapter.title = song.title;
                        chapter.artist = song.artist;
                        chapter.author = song.author;
                        value_routes = song.value_routes;
                    }
                    Err(error) => unresolved.push((index, error)),
                }
                chapter.target = SplitTarget::Remote {
                    feed_guid: feed_guid.to_owned(),
                    item_guid,
                    value_routes,
                };
            }
            splits.push(chapter);
        }
        Ok(EpisodeSplits { splits, unresolved })
    }
}

fn remote_percentage(split: &IngestValueTimeSplit) -> Result<Option<u32>, ()> {
    let Some(percentage) = split.remote_percentage else {
        return Ok(None);
    };
    if !(percentage.is_finite() && (0.0..=100.0).contains(&percentage)) {
        return Err(());
    }
    // The range check above keeps the value in 0 to 100.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(Some(percentage.round() as u32))
}
