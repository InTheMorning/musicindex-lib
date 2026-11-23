//! The chapter songs of an episode file (musicindex ADR 0093 R-TAG-4).

use std::fmt;

use anyhow::{Result, anyhow};
use id3::frame::{Chapter, ExtendedText, TableOfContents};
use id3::{Content, Frame, Tag, TagLike};
use serde::{Deserialize, Serialize};

use crate::frames;

/// The highest number of entries in one `CTOC` frame (`ID3v2` chapter addendum).
pub const MAX_CHAPTERS: usize = 255;

/// The element ID of the top-level `CTOC` frame (R-TAG-4 item 3).
pub const TOC_ELEMENT_ID: &str = "toc";

/// The value of a `CHAP` byte offset that is not used (`ID3v2` chapter addendum).
const NO_OFFSET: u32 = u32::MAX;

/// One `podcast:valueTimeSplit` of an episode, with the song data that the
/// caller gives.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    /// The attribute `startTime`, in seconds.
    pub start_time: f64,
    /// The attribute `duration`, in seconds.
    pub duration: f64,
    /// The attribute `remotePercentage`. `None` when the attribute is missing.
    pub remote_percentage: Option<u32>,
    /// The song title. The caller gives it.
    pub title: Option<String>,
    /// The song artist. The caller gives it.
    pub artist: Option<String>,
    /// The `itunes:author` of the song feed. The caller gives it.
    pub author: Option<String>,
    /// The `podcast:remoteItem` or the recipients of the split.
    pub target: SplitTarget,
}

/// The payment target of a [`Split`].
#[derive(Clone, Debug, PartialEq)]
pub enum SplitTarget {
    /// A remote split: a `podcast:remoteItem` that points to a song.
    Remote {
        /// The attribute `feedGuid`.
        feed_guid: String,
        /// The attribute `itemGuid`. `None` when the attribute is missing.
        /// The writer does not make a GUID (R-TAG-4 item 5).
        item_guid: Option<String>,
        /// The value block of the song item, or of the song feed channel when
        /// the item has none, in the value form of `TXXX:Podcast Value
        /// Routes` (musicindex ADR 0093 R-TAG-6 item 1). `None` when the
        /// writer cannot get the block (R-TAG-6 item 2). The app reads the
        /// song feed. This crate does not read a feed or make routes
        /// (R-TAG-6 item 4).
        value_routes: Option<serde_json::Value>,
    },
    /// A local split: its own `podcast:valueRecipient` elements, in the value
    /// form of `TXXX:Podcast Value Routes`.
    Local {
        /// The recipients as a JSON array.
        value_routes: serde_json::Value,
    },
}

/// One entry of the JSON array of `TXXX:Podcast Chapters` (R-TAG-4 item 4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChapterEntry {
    /// The start in milliseconds.
    pub start_ms: u32,
    /// The end in milliseconds.
    pub end_ms: u32,
    /// The song title, or `None`.
    pub title: Option<String>,
    /// The song artist, or `None`.
    pub artist: Option<String>,
    /// The `itunes:author` of the song feed. The field is optional in the JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The `feedGuid` of the `podcast:remoteItem`, or `None` for a local split.
    pub feed_guid: Option<String>,
    /// The `itemGuid` of the `podcast:remoteItem`, or `None`.
    pub item_guid: Option<String>,
    /// The attribute `remotePercentage`. It is 100 when the attribute is missing.
    pub remote_percentage: u32,
    /// The recipients of a local split, or the routes of the song of a remote
    /// split (musicindex ADR 0093 R-TAG-6). `None` gives JSON `null`.
    pub value_routes: Option<serde_json::Value>,
}

/// One `CHAP` frame that the reader found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterFrame {
    /// The element ID, for example `chp0`.
    pub element_id: String,
    /// The start in milliseconds.
    pub start_ms: u32,
    /// The end in milliseconds.
    pub end_ms: u32,
    /// The `TIT2` subframe.
    pub title: Option<String>,
    /// The `TXXX:Podcast Feed Guid` subframe.
    pub feed_guid: Option<String>,
    /// The `TXXX:Podcast Item Guid` subframe.
    pub item_guid: Option<String>,
}

/// The error for an episode with more splits than one `CTOC` frame can list
/// (R-TAG-4 item 3). The writer writes no frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooManyChapters {
    /// The number of splits of the episode.
    pub count: usize,
}

impl fmt::Display for TooManyChapters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the episode has {} splits, and one CTOC frame can list {MAX_CHAPTERS} \
             (musicindex ADR 0093 R-TAG-4 item 3)",
            self.count
        )
    }
}

impl std::error::Error for TooManyChapters {}

/// The chapter list of one write, in time order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChapterFrames {
    entries: Vec<ChapterEntry>,
    json: Option<String>,
}

impl ChapterFrames {
    /// Makes the chapter list of `splits`.
    ///
    /// # Errors
    ///
    /// Returns [`TooManyChapters`] for more than 255 splits, and an error for
    /// a time that is not a finite, positive number of seconds.
    pub(crate) fn from_splits(splits: &[Split]) -> Result<Self> {
        if splits.len() > MAX_CHAPTERS {
            return Err(TooManyChapters {
                count: splits.len(),
            }
            .into());
        }
        let mut entries = splits
            .iter()
            .map(chapter_entry)
            .collect::<Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| (entry.start_ms, entry.end_ms));
        let json = if entries.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&entries)?)
        };
        Ok(Self { entries, json })
    }

    /// The JSON of `TXXX:Podcast Chapters`, or `None` for an episode with no
    /// split.
    pub(crate) fn json(&self) -> Option<&str> {
        self.json.as_deref()
    }

    /// Removes each `CHAP`, `CTOC` and `TXXX:Podcast Chapters` frame of `tag`,
    /// then adds the frames of this list. An empty list adds no frame.
    pub(crate) fn replace_id3_frames(&self, tag: &mut Tag) {
        let chapters_description = frames::descriptor(frames::CHAPTERS);
        tag.frames_vec_mut().retain(|frame| {
            let is_chapters_text = frame
                .content()
                .extended_text()
                .is_some_and(|text| text.description.eq_ignore_ascii_case(chapters_description));
            !matches!(frame.id(), "CHAP" | "CTOC") && !is_chapters_text
        });
        let Some(json) = &self.json else {
            return;
        };
        let element_ids = (0..self.entries.len())
            .map(|index| format!("chp{index}"))
            .collect::<Vec<_>>();
        let frames_vec = tag.frames_vec_mut();
        for (entry, element_id) in self.entries.iter().zip(&element_ids) {
            frames_vec.push(chapter_frame(entry, element_id));
        }
        frames_vec.push(Frame::with_content(
            "CTOC",
            Content::TableOfContents(TableOfContents {
                element_id: TOC_ELEMENT_ID.to_owned(),
                top_level: true,
                ordered: true,
                elements: element_ids,
                frames: Vec::new(),
            }),
        ));
        frames_vec.push(text_frame(chapters_description, json));
    }
}

fn chapter_entry(split: &Split) -> Result<ChapterEntry> {
    if !split.duration.is_finite() || split.duration < 0.0 {
        return Err(anyhow!(
            "the split duration {} is not valid",
            split.duration
        ));
    }
    let (feed_guid, item_guid, value_routes) = match &split.target {
        SplitTarget::Remote {
            feed_guid,
            item_guid,
            value_routes,
        } => (
            Some(feed_guid.clone()),
            item_guid.clone(),
            value_routes.clone(),
        ),
        SplitTarget::Local { value_routes } => (None, None, Some(value_routes.clone())),
    };
    Ok(ChapterEntry {
        start_ms: seconds_to_ms(split.start_time)?,
        end_ms: seconds_to_ms(split.start_time + split.duration)?,
        title: split.title.clone(),
        artist: split.artist.clone(),
        author: split.author.clone(),
        feed_guid,
        item_guid,
        remote_percentage: split.remote_percentage.unwrap_or(100),
        value_routes,
    })
}

/// Converts `seconds` to milliseconds, rounded to the nearest millisecond
/// (R-TAG-4 item 2).
fn seconds_to_ms(seconds: f64) -> Result<u32> {
    let ms = (seconds * 1000.0).round();
    if !ms.is_finite() || ms < 0.0 || ms > f64::from(u32::MAX) {
        return Err(anyhow!("the split time {seconds} s is not valid"));
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is a whole number in the range of u32"
    )]
    Ok(ms as u32)
}

/// The `CHAP` frame of `entry` (R-TAG-4 item 2). A local split has no GUID
/// subframe. A remote split with no item GUID has no item GUID subframe.
fn chapter_frame(entry: &ChapterEntry, element_id: &str) -> Frame {
    let mut subframes = Vec::new();
    if let Some(title) = &entry.title {
        subframes.push(Frame::text("TIT2", title.clone()));
    }
    if let Some(feed_guid) = &entry.feed_guid {
        subframes.push(text_frame(frames::descriptor(frames::FEED_GUID), feed_guid));
    }
    if let Some(item_guid) = &entry.item_guid {
        subframes.push(text_frame(frames::descriptor(frames::ITEM_GUID), item_guid));
    }
    Frame::with_content(
        "CHAP",
        Content::Chapter(Chapter {
            element_id: element_id.to_owned(),
            start_time: entry.start_ms,
            end_time: entry.end_ms,
            start_offset: NO_OFFSET,
            end_offset: NO_OFFSET,
            frames: subframes,
        }),
    )
}

fn text_frame(description: &str, value: &str) -> Frame {
    Frame::with_content(
        "TXXX",
        Content::ExtendedText(ExtendedText {
            description: description.to_owned(),
            value: value.to_owned(),
        }),
    )
}

/// The `CHAP` frames of `tag`, in the sequence of the tag.
pub(crate) fn read_chapter_frames(tag: &Tag) -> Vec<ChapterFrame> {
    tag.frames()
        .filter_map(|frame| match frame.content() {
            Content::Chapter(chapter) => Some(ChapterFrame {
                element_id: chapter.element_id.clone(),
                start_ms: chapter.start_time,
                end_ms: chapter.end_time,
                title: chapter.frames.iter().find_map(|sub| {
                    (sub.id() == "TIT2")
                        .then(|| sub.content().text().map(str::to_owned))
                        .flatten()
                }),
                feed_guid: subframe_text(&chapter.frames, frames::FEED_GUID),
                item_guid: subframe_text(&chapter.frames, frames::ITEM_GUID),
            }),
            _ => None,
        })
        .collect()
}

/// The element IDs of the top-level `CTOC` frame of `tag`, in its sequence.
pub(crate) fn read_toc(tag: &Tag) -> Vec<String> {
    tag.frames()
        .find_map(|frame| match frame.content() {
            Content::TableOfContents(toc) if toc.top_level => Some(toc.elements.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn subframe_text(subframes: &[Frame], label: &str) -> Option<String> {
    let description = frames::descriptor(label);
    subframes.iter().find_map(|sub| {
        sub.content()
            .extended_text()
            .filter(|text| text.description.eq_ignore_ascii_case(description))
            .map(|text| text.value.trim_end_matches('\0').to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(start_time: f64, duration: f64) -> Split {
        Split {
            start_time,
            duration,
            remote_percentage: None,
            title: None,
            artist: None,
            author: None,
            target: SplitTarget::Remote {
                feed_guid: "feed".into(),
                item_guid: None,
                value_routes: None,
            },
        }
    }

    #[test]
    fn times_round_to_the_nearest_millisecond() {
        let frames = ChapterFrames::from_splits(&[remote(1.0004, 2.0006)]).expect("chapters");
        assert_eq!(frames.entries[0].start_ms, 1000);
        assert_eq!(frames.entries[0].end_ms, 3001);
    }

    #[test]
    fn entries_are_in_time_order() {
        let frames =
            ChapterFrames::from_splits(&[remote(60.0, 10.0), remote(5.0, 10.0)]).expect("chapters");
        let starts = frames
            .entries
            .iter()
            .map(|entry| entry.start_ms)
            .collect::<Vec<_>>();
        assert_eq!(starts, vec![5000, 60_000]);
    }

    #[test]
    fn a_negative_time_is_an_error() {
        assert!(ChapterFrames::from_splits(&[remote(-1.0, 10.0)]).is_err());
        assert!(ChapterFrames::from_splits(&[remote(1.0, -10.0)]).is_err());
        assert!(ChapterFrames::from_splits(&[remote(f64::NAN, 10.0)]).is_err());
    }

    #[test]
    fn a_missing_remote_percentage_is_100() {
        let frames = ChapterFrames::from_splits(&[remote(0.0, 1.0)]).expect("chapters");
        assert_eq!(frames.entries[0].remote_percentage, 100);
    }
}
