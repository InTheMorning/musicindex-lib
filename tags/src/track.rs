use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use lofty::file::TaggedFile;
use lofty::picture::{Picture, PictureType};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, ItemValue, Tag, TagType};
use serde::Serialize;

use crate::audio::convert_old_frames;
use crate::id3v2::{TagProblems, read_id3v2};

pub const MUSICINDEX_VOCABULARY: &[MusicIndexFrame] = &[
    MusicIndexFrame::new("Publisher", "TXXX:V4V_PUBLISHER"),
    MusicIndexFrame::new("Feed Guid", "TXXX:Podcast Feed Guid"),
    MusicIndexFrame::new("Track Guid", "TXXX:Podcast Item Guid"),
    MusicIndexFrame::new("Nostr Handle", "TXXX:RSS Nostr Handle"),
    MusicIndexFrame::new("Website", "WOAR"),
    MusicIndexFrame::new("Description", "COMM:Podcast Description"),
    MusicIndexFrame::new("Contributors", "TXXX:Podcast Contributors"),
    MusicIndexFrame::new("Value Routes", "TXXX:Podcast Value Routes"),
    MusicIndexFrame::new("MusicBrainz Recording", "UFID:http://musicbrainz.org"),
    MusicIndexFrame::new("MusicBrainz Release", "TXXX:MusicBrainz Album Id"),
    MusicIndexFrame::new(
        "MusicBrainz Release Group",
        "TXXX:MusicBrainz Release Group Id",
    ),
    MusicIndexFrame::new("Release Country", "TXXX:MusicBrainz Album Release Country"),
    MusicIndexFrame::new("Release Status", "TXXX:MusicBrainz Album Status"),
    MusicIndexFrame::new("Barcode", "TXXX:BARCODE"),
    MusicIndexFrame::new("Release Type", "TXXX:MusicBrainz Album Type"),
    MusicIndexFrame::new("Disc Subtitle", "TSST"),
    MusicIndexFrame::new("Image", "TXXX:Podcast Image"),
    MusicIndexFrame::new("Chapters", "TXXX:Podcast Chapters"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicIndexFrame {
    pub canonical: &'static str,
    pub frame_label: &'static str,
}

impl MusicIndexFrame {
    const fn new(canonical: &'static str, frame_label: &'static str) -> Self {
        Self {
            canonical,
            frame_label,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackTags {
    pub musicindex: Vec<TagText>,
    pub tags: Vec<TagText>,
    pub duration: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagText {
    pub key: String,
    pub value: String,
}

impl TagText {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

impl TrackTags {
    #[must_use]
    pub fn musicindex_value(&self, key: &str) -> Option<&str> {
        self.musicindex
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.value.as_str())
    }

    pub fn set_musicindex_value(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(item) = self.musicindex.iter_mut().find(|item| item.key == key) {
            item.value = value;
            return;
        }
        self.musicindex.push(TagText::new(key, value));
    }

    pub fn remove_musicindex_value(&mut self, key: &str) {
        self.musicindex.retain(|item| item.key != key);
    }
}

/// Reads the tags of `path` for the now-playing display. The binary picture
/// data is not kept.
///
/// # Errors
///
/// Returns an error when the file cannot be read or its tag cannot be parsed.
pub fn read_tags(path: &Path) -> Result<TrackTags> {
    read_tags_checked(path).map(|(tags, _)| tags)
}

/// Reads the tags of `path` as [`read_tags`] does, and gives the problems of
/// its ID3v2 tag (musicindex ADR 0093 R-TAG-7 item 3).
///
/// The display read of an ID3v2 tag uses the frame loop of
/// [`crate::id3v2::read_id3v2`]. It keeps each good frame before and after a
/// bad frame, and it shows a frame of an older version as its ID3v2.4
/// equivalent. When the file has an ID3v2 tag, the read does not show the
/// ID3v1 tag, because a write removes it (R-TAG-8 item 5). When the tag
/// header is bad, the read shows no ID3v2 frame and no ID3v1 data.
///
/// # Errors
///
/// Returns an error when the file cannot be read or `lofty` cannot parse it.
pub fn read_tags_checked(path: &Path) -> Result<(TrackTags, TagProblems)> {
    let (tagged_file, problems) = read_tagged_file(path)?;
    Ok((track_tags(&tagged_file), problems))
}

/// Reads the tags and the display picture of `path` in one read of the
/// file.
///
/// The tags are the same as the tags of [`read_tags`]. The picture is the
/// front cover, else the first picture (ADR 0008 §The Embedded Image).
///
/// # Errors
///
/// Returns an error when `lofty` cannot read the file.
pub fn read_tags_with_picture(path: &Path) -> Result<(TrackTags, Option<Vec<u8>>)> {
    let (mut tagged_file, _) = read_tagged_file(path)?;
    let tags = track_tags(&tagged_file);
    let picture = take_picture(&mut tagged_file);
    Ok((tags, picture))
}

/// The length of the audio of `path` that `lofty` measures, the same length
/// as [`TrackTags::duration`]. `None` when `lofty` cannot read the file or
/// measures no length (musicindex ADR 0093 R-TAG-11 item 3).
pub(crate) fn measured_length(path: &Path) -> Option<Duration> {
    let (tagged_file, _) = read_tagged_file(path).ok()?;
    nonzero_duration(tagged_file.properties().duration())
}

/// Reads `path` with `lofty` after the frame loop of R-TAG-7.
///
/// A file with no ID3v2 tag, or with a correct ID3v2.4 tag and no ID3v1 tag,
/// goes to `lofty` without a change. A correct tag has no bad frame, no plain
/// frame size and no frame of an older version. For each other file with an ID3v2 tag,
/// `lofty` reads a copy in memory: an ID3v2.4 tag of the good frames, then
/// the bytes after the old tag, with no ID3v1 tag.
fn read_tagged_file(path: &Path) -> Result<(TaggedFile, TagProblems)> {
    let read = read_id3v2(path)?;
    let context = || format!("read tags from {}", path.display());
    if read.problems.bad_tag.is_some() {
        let mut tagged_file = lofty::read_from_path(path).with_context(context)?;
        tagged_file.remove(TagType::Id3v2);
        tagged_file.remove(TagType::Id3v1);
        return Ok((tagged_file, read.problems));
    }
    let mut tag = read.tag.clone();
    let old_frames = convert_old_frames(&mut tag, read.version);
    let correct = read.version.is_none()
        || (read.version == Some(4)
            && !read.has_id3v1
            && !read.plain_frame_sizes
            && read.problems.bad_frames.is_empty()
            && old_frames.is_empty());
    if correct {
        let tagged_file = lofty::read_from_path(path).with_context(context)?;
        return Ok((tagged_file, read.problems));
    }

    let bytes = std::fs::read(path).with_context(context)?;
    let start = usize::try_from(read.tag_len)
        .ok()
        .filter(|start| *start <= bytes.len())
        .with_context(context)?;
    let end = if read.has_id3v1 && bytes.len() >= start + ID3V1_LEN {
        bytes.len() - ID3V1_LEN
    } else {
        bytes.len()
    };
    let mut copy = Vec::with_capacity(bytes.len());
    if tag.frames().next().is_some() {
        tag.write_to(&mut copy, id3::Version::Id3v24)
            .with_context(context)?;
    }
    copy.extend_from_slice(&bytes[start..end]);
    let tagged_file = Probe::new(Cursor::new(copy))
        .guess_file_type()
        .with_context(context)?
        .read()
        .with_context(context)?;
    Ok((tagged_file, read.problems))
}

/// The length in bytes of an ID3v1 tag at the end of a file.
const ID3V1_LEN: usize = 128;

/// Selects the display picture: the front cover, else the first picture
/// (ADR 0008 §The Embedded Image). The tags are examined in their order.
fn select_picture<'a, I>(tags: I) -> Option<(usize, usize)>
where
    I: IntoIterator<Item = &'a [Picture]>,
    I::IntoIter: Clone,
{
    let tags = tags.into_iter();
    let front = tags.clone().enumerate().find_map(|(tag_index, pictures)| {
        pictures
            .iter()
            .position(|picture| picture.pic_type() == PictureType::CoverFront)
            .map(|index| (tag_index, index))
    });
    front.or_else(|| {
        tags.enumerate()
            .find_map(|(tag_index, pictures)| (!pictures.is_empty()).then_some((tag_index, 0)))
    })
}

/// Removes the selected picture from `tagged_file` and gives its data. The
/// removal prevents a copy of the data.
fn take_picture(tagged_file: &mut TaggedFile) -> Option<Vec<u8>> {
    let (tag_index, index) =
        select_picture(tagged_file.tags().iter().map(lofty::tag::Tag::pictures))?;
    let tag_type = tagged_file.tags().get(tag_index)?.tag_type();
    let tag = tagged_file.tag_mut(tag_type)?;
    (index < tag.pictures().len()).then(|| tag.remove_picture(index).into_data())
}

fn track_tags(tagged_file: &TaggedFile) -> TrackTags {
    let duration = nonzero_duration(tagged_file.properties().duration());
    let mut track_tags = TrackTags {
        musicindex: Vec::new(),
        tags: Vec::new(),
        duration,
    };

    for tag in tagged_file.tags() {
        collect_tag_items(tag, &mut track_tags);
        collect_pictures(tag, &mut track_tags);
    }

    track_tags
}

fn nonzero_duration(duration: Duration) -> Option<Duration> {
    if duration.is_zero() {
        None
    } else {
        Some(duration)
    }
}

fn collect_tag_items(tag: &Tag, track_tags: &mut TrackTags) {
    for item in tag.items() {
        let key = item_key(tag, item.key(), item.description());
        if is_transcript_key(&key) {
            continue;
        }

        let value = render_item_value(&key, item.value());
        push_tag_text(track_tags, key, value);
    }
}

fn collect_pictures(tag: &Tag, track_tags: &mut TrackTags) {
    for picture in tag.pictures() {
        track_tags
            .tags
            .push(TagText::new("APIC", render_picture_summary(picture)));
    }
}

fn item_key(tag: &Tag, key: &ItemKey, description: &str) -> String {
    match key {
        ItemKey::Unknown(unknown) => unknown.clone(),
        ItemKey::Comment if !description.is_empty() => format!("COMM:{description}"),
        _ => key
            .map_key(tag.tag_type(), true)
            .map_or_else(|| format!("{key:?}"), str::to_string),
    }
}

fn push_tag_text(track_tags: &mut TrackTags, key: String, value: String) {
    if let Some(canonical) = canonical_musicindex_key(&key) {
        track_tags.musicindex.push(TagText::new(canonical, value));
    } else {
        track_tags.tags.push(TagText::new(key, value));
    }
}

fn canonical_musicindex_key(key: &str) -> Option<&'static str> {
    let normalized = normalize_key(key);
    MUSICINDEX_VOCABULARY
        .iter()
        .find(|entry| normalize_key(frame_match_key(entry.frame_label)) == normalized)
        .map(|entry| entry.canonical)
}

fn frame_match_key(frame_label: &str) -> &str {
    frame_label
        .rsplit_once(':')
        .map_or(frame_label, |(_, key)| key)
}

fn normalize_key(key: &str) -> String {
    key.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn is_transcript_key(key: &str) -> bool {
    let normalized = normalize_key(frame_match_key(key));
    normalized == "USLT" || normalized == "PODCAST TRANSCRIPT"
}

fn render_item_value(key: &str, value: &ItemValue) -> String {
    if is_binary_summary_key(key) {
        return binary_summary(key, value);
    }

    match value {
        ItemValue::Text(text) | ItemValue::Locator(text) => text.clone(),
        ItemValue::Binary(binary) => format!("{} bytes", binary.len()),
    }
}

fn is_binary_summary_key(key: &str) -> bool {
    let normalized = normalize_key(key);
    normalized == "APIC" || normalized == "PRIV" || normalized == "UFID"
}

fn binary_summary(key: &str, value: &ItemValue) -> String {
    match value {
        ItemValue::Binary(binary) => format!("{} bytes", binary.len()),
        ItemValue::Text(text) | ItemValue::Locator(text) => {
            if text.is_empty() {
                "0 bytes".to_string()
            } else {
                format!("{key} text, {} bytes", text.len())
            }
        }
    }
}

fn render_picture_summary(picture: &Picture) -> String {
    let mime_type = picture
        .mime_type()
        .map_or_else(|| "unknown".to_string(), ToString::to_string);
    format!("{mime_type}, {}", render_kib(picture.data().len()))
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a display value with one decimal"
)]
fn render_kib(bytes: usize) -> String {
    format!("{:.1} KB", bytes as f64 / 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_normalizes_musicindex_keys() {
        assert_eq!(
            canonical_musicindex_key("Podcast Feed Guid"),
            Some("Feed Guid")
        );
        assert_eq!(
            canonical_musicindex_key("PODCAST   FEED   GUID"),
            Some("Feed Guid")
        );
        assert_eq!(
            canonical_musicindex_key("Podcast Value Routes"),
            Some("Value Routes")
        );
        assert_eq!(
            canonical_musicindex_key("Podcast Chapters"),
            Some("Chapters")
        );
    }

    /// Musicindex ADR 0093 R-TAG-2 item 2: the reader does not accept an old
    /// name.
    #[test]
    fn tags_does_not_read_old_names() {
        assert_eq!(canonical_musicindex_key("MusicIndex Feed Guid"), None);
        assert_eq!(canonical_musicindex_key("MusicIndex Track Guid"), None);
        assert_eq!(canonical_musicindex_key("MusicIndex Value Routes"), None);
    }

    #[test]
    fn tags_identifies_transcript_keys() {
        assert!(is_transcript_key("USLT"));
        assert!(is_transcript_key("SYLT:Podcast Transcript"));
        assert!(is_transcript_key("Podcast Transcript"));
        assert!(!is_transcript_key("Podcast Description"));
    }

    #[test]
    fn tags_summarizes_binary_payload_keys() {
        let binary = ItemValue::Binary(vec![0, 1, 2, 3]);

        assert_eq!(render_item_value("PRIV", &binary), "4 bytes");
        assert_eq!(render_item_value("UFID", &binary), "4 bytes");
        assert_eq!(render_item_value("APIC", &binary), "4 bytes");
    }
}
