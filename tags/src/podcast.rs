//! The song and episode tags of musicindex ADR 0086 R-TAG-3 and R-TAG-4.

use std::path::Path;

use anyhow::{Result, anyhow};

use crate::audio::{self, AudioTags, Id3v24Edit, TagWriteResult};
use crate::chapters::{self, ChapterEntry, ChapterFrame, ChapterFrames, Split};
use crate::format::AudioFormat;
use crate::frames;
use crate::id3v2::{TagProblems, read_id3v2};
use crate::source::ReferenceSource;

/// The GUIDs of a song file (R-TAG-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongTags {
    /// The `podcast:guid` of the song feed.
    pub feed_guid: String,
    /// The item `guid` of the song.
    pub item_guid: String,
}

/// The tags of an episode file (R-TAG-4).
#[derive(Clone, Debug, PartialEq)]
pub struct EpisodeTags {
    /// The `podcast:guid` of the show feed.
    pub feed_guid: String,
    /// The item `guid` of the episode.
    pub item_guid: String,
    /// The value block of the episode in the value form of
    /// `TXXX:Podcast Value Routes`. `None` keeps the frame of the file.
    pub value_routes: Option<String>,
    /// The `podcast:valueTimeSplit` elements of the episode, in any order.
    pub splits: Vec<Split>,
}

/// The podcast frames of a file, read with the new names only (R-TAG-2
/// item 2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PodcastTags {
    /// `TXXX:Podcast Feed Guid`.
    pub feed_guid: Option<String>,
    /// `TXXX:Podcast Item Guid`.
    pub item_guid: Option<String>,
    /// `TXXX:Podcast Value Routes`.
    pub value_routes: Option<String>,
    /// `TXXX:Podcast Image`.
    pub image: Option<String>,
    /// `TXXX:Podcast Contributors`.
    pub contributors: Option<String>,
    /// `TXXX:Podcast License`.
    pub license: Option<String>,
    /// `COMM:Podcast Description`.
    pub description: Option<String>,
    /// `COMM:Podcast Feed Description`.
    pub feed_description: Option<String>,
    /// `USLT:Podcast Transcript`.
    pub transcript: Option<String>,
    /// `TXXX:Podcast Chapters`. [`PodcastTags::chapters`] parses it.
    pub chapters_json: Option<String>,
    /// The `CHAP` frames of an MP3 file, in the sequence of the tag.
    pub chapter_frames: Vec<ChapterFrame>,
    /// The element IDs of the top-level `CTOC` frame of an MP3 file.
    pub toc: Vec<String>,
    /// Each tag of the file, as [`audio::read_audio_tags`] gives it.
    pub audio: AudioTags,
    /// The problems of the ID3v2 tag of an MP3 file (musicindex ADR 0090
    /// R-TAG-7 item 2).
    pub problems: TagProblems,
}

impl PodcastTags {
    /// The entries of `TXXX:Podcast Chapters`. A file with no such frame
    /// gives an empty list.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame does not hold a JSON array of entries.
    pub fn chapters(&self) -> serde_json::Result<Vec<ChapterEntry>> {
        self.chapters_json
            .as_deref()
            .map_or_else(|| Ok(Vec::new()), serde_json::from_str)
    }
}

impl SongTags {
    /// The edits of the two GUID frames.
    #[must_use]
    pub fn edits(&self) -> Vec<Id3v24Edit> {
        vec![
            edit(frames::FEED_GUID, &self.feed_guid),
            edit(frames::ITEM_GUID, &self.item_guid),
        ]
    }
}

/// Writes the GUIDs of `song` and `edits` into `path` in one write. A GUID
/// of `song` replaces an edit of `edits` with the same frame.
///
/// # Errors
///
/// Returns an error when a GUID is empty, or for each error of
/// [`audio::write_id3v24_edits`].
pub fn write_song_tags(
    path: &Path,
    song: &SongTags,
    edits: &[Id3v24Edit],
    source: &dyn ReferenceSource,
) -> Result<TagWriteResult> {
    let edits = merged_edits(edits, song.edits())?;
    audio::write_tags(path, &edits, None, source)
}

/// Writes the tags of `episode` and `edits` into `path` in one write
/// (R-TAG-4). An MP3 file gets one `CHAP` frame for each split, the `CTOC`
/// frame and `TXXX:Podcast Chapters`. A different format gets only
/// `TXXX:Podcast Chapters` (R-TAG-1 item 5). The write removes the chapter
/// frames of an earlier write first. An episode with no split gets no
/// chapter frame.
///
/// # Errors
///
/// Returns [`chapters::TooManyChapters`] for more than 255 splits, and then
/// writes nothing. Also returns an error for an empty GUID, for a split time
/// that is not valid, and for each error of [`audio::write_id3v24_edits`].
pub fn write_episode_tags(
    path: &Path,
    episode: &EpisodeTags,
    edits: &[Id3v24Edit],
    source: &dyn ReferenceSource,
) -> Result<TagWriteResult> {
    let chapters = ChapterFrames::from_splits(&episode.splits)?;
    let mut own = vec![
        edit(frames::FEED_GUID, &episode.feed_guid),
        edit(frames::ITEM_GUID, &episode.item_guid),
    ];
    if let Some(value_routes) = &episode.value_routes {
        own.push(edit(frames::VALUE_ROUTES, value_routes));
    }
    let edits = merged_edits(edits, own)?;
    audio::write_tags(path, &edits, Some(&chapters), source)
}

/// Reads the podcast frames of `path` with the new names only.
///
/// # Errors
///
/// Returns an error when the file cannot be read or its tag cannot be parsed.
pub fn read_podcast_tags(path: &Path) -> Result<PodcastTags> {
    let is_mp3 = !matches!(
        AudioFormat::detect_from_file(path),
        Ok(AudioFormat::Flac
            | AudioFormat::OggVorbis
            | AudioFormat::OggOpus
            | AudioFormat::Mp4
            | AudioFormat::Wav)
    );
    if !is_mp3 {
        return Ok(podcast_tags(audio::read_audio_tags(path)?));
    }
    let read = read_id3v2(path)?;
    let mut tags = podcast_tags(audio::audio_tags_from_id3(&read.tag));
    tags.chapter_frames = chapters::read_chapter_frames(&read.tag);
    tags.toc = chapters::read_toc(&read.tag);
    tags.problems = read.problems;
    Ok(tags)
}

fn podcast_tags(audio: AudioTags) -> PodcastTags {
    let value = |label: &str| field_value(&audio, label);
    PodcastTags {
        feed_guid: value(frames::FEED_GUID),
        item_guid: value(frames::ITEM_GUID),
        value_routes: value(frames::VALUE_ROUTES),
        image: value(frames::IMAGE),
        contributors: value(frames::CONTRIBUTORS),
        license: value(frames::LICENSE),
        description: value(frames::DESCRIPTION),
        feed_description: value(frames::FEED_DESCRIPTION),
        transcript: value(frames::TRANSCRIPT_TEXT),
        chapters_json: value(frames::CHAPTERS),
        chapter_frames: Vec::new(),
        toc: Vec::new(),
        audio,
        problems: TagProblems::default(),
    }
}

/// The value of the first field with the label `label`, with any letter
/// case. A Vorbis comment holds its key in capital letters.
fn field_value(tags: &AudioTags, label: &str) -> Option<String> {
    tags.fields
        .iter()
        .find(|field| field.frame_id.eq_ignore_ascii_case(label))
        .map(|field| field.value.clone())
}

fn edit(frame_label: &str, value: &str) -> Id3v24Edit {
    Id3v24Edit {
        frame_label: frame_label.to_owned(),
        value: value.to_owned(),
    }
}

/// `edits` without each edit that `own` replaces, then `own`. One label has
/// one edit, so a Vorbis comment gets one value for each key.
fn merged_edits(edits: &[Id3v24Edit], own: Vec<Id3v24Edit>) -> Result<Vec<Id3v24Edit>> {
    if let Some(empty) = own.iter().find(|edit| edit.value.trim().is_empty()) {
        return Err(anyhow!("the value of {} is empty", empty.frame_label));
    }
    let mut merged = edits
        .iter()
        .filter(|edit| {
            !own.iter()
                .any(|mine| mine.frame_label.eq_ignore_ascii_case(&edit.frame_label))
        })
        .cloned()
        .collect::<Vec<_>>();
    merged.extend(own);
    Ok(merged)
}
