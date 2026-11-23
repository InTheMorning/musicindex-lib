use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use id3::frame::{
    Comment, ExtendedLink, ExtendedText, InvolvedPeopleList, InvolvedPeopleListItem, Lyrics,
    Picture, PictureType, SynchronisedLyrics, SynchronisedLyricsType, TimestampFormat,
    UniqueFileIdentifier,
};
use id3::{Content, Frame, Tag, TagLike, Version};

use crate::chapters::ChapterFrames;
use crate::format::AudioFormat;
use crate::frames;
use crate::id3v2::{BadFrame, BadTag, TagProblems, read_id3v2};
use crate::source::ReferenceSource;

const WRITABLE_TEXT_FRAMES: &[&str] = &[
    "TALB", "TBPM", "TCOM", "TCON", "TCOP", "TDEN", "TDLY", "TDOR", "TDRC", "TDRL", "TDTG", "TENC",
    "TEXT", "TFLT", "TIT1", "TIT2", "TIT3", "TKEY", "TLAN", "TLEN", "TMED", "TMOO", "TOAL", "TOFN",
    "TOLY", "TOPE", "TOWN", "TPE1", "TPE2", "TPE3", "TPE4", "TPOS", "TPRO", "TPUB", "TRCK", "TRSN",
    "TRSO", "TSOA", "TSOP", "TSOT", "TSRC", "TSSE", "TSST", "TXXX", "TYER",
];
const WRITABLE_URL_FRAMES: &[&str] = &[
    "WCOM", "WCOP", "WOAF", "WOAR", "WOAS", "WORS", "WPAY", "WPUB", "WXXX",
];

/// HitIt ADR 0080 Decision 2: the two frame labels that hold a website. A write
/// treats them as one family: a value under one label can replace an
/// earlier value under either label, so a fact that moved frame, or a
/// repeated write, leaves no stale copy (Decision 6).
const WEBSITE_FRAME_LABELS: [&str; 2] = ["WOAR", "WOAF"];

/// HitIt ADR 0080 Decision 5: the two frame labels that hold a description. A
/// track holds at most one of them, so a write that sets either one clears
/// both first.
const DESCRIPTION_FRAME_LABELS: [&str; 2] = [frames::DESCRIPTION, frames::FEED_DESCRIPTION];

/// The frame that holds the URL of the RSS license when the file holds no
/// different license URL (HitIt ADR 0080 Decision 11).
pub const LICENSE_URL_FRAME: &str = "WCOP";

/// The frame that holds the text of the RSS license, for example
/// `cc-by-4.0` (HitIt ADR 0080 Decision 11).
pub const LICENSE_NAME_FRAME: &str = frames::LICENSE;

/// The frame that holds more than one license URL (HitIt ADR 0080 Decisions 8 and
/// 11). It holds them in one value, joined with the separator below.
pub const LICENSE_LIST_FRAME: &str = "TXXX:LICENSE";

/// The separator of `join_values` in HitIt `src/metadata.rs`. A
/// `TXXX:LICENSE` value joins its URLs with it.
const LICENSE_LIST_SEPARATOR: &str = " · ";

/// The result of `resolve_license_edits`.
#[derive(Debug, Default, PartialEq, Eq)]
struct LicenseResolution {
    /// The edits that the writer applies. The license URLs of the write are
    /// in one `WCOP` edit, or in one `TXXX:LICENSE` edit for more than one.
    edits: Vec<Id3v24Edit>,
    /// `true` when `edits` holds a license edit. The writer then removes each
    /// `WCOP` and `TXXX:LICENSE` value of the file first.
    replaces_license: bool,
}

fn license_urls_equal(left: &str, right: &str) -> bool {
    left.trim()
        .trim_end_matches('/')
        .eq_ignore_ascii_case(right.trim().trim_end_matches('/'))
}

/// The URLs of one `TXXX:LICENSE` value. This reverses `join_values`.
#[must_use]
pub fn split_license_list(value: &str) -> Vec<String> {
    value
        .split(LICENSE_LIST_SEPARATOR)
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// HitIt ADR 0080 Decisions 6, 8 and 11: the license URLs of `edits` replace each
/// license URL of the file. One URL goes in `WCOP`. More than one URL goes in
/// one `TXXX:LICENSE` value, because ID3v2.4 permits one `WCOP` frame. The
/// result is the same for a second write.
fn resolve_license_edits(edits: &[Id3v24Edit]) -> LicenseResolution {
    let mut urls = Vec::<String>::new();
    let mut resolution = LicenseResolution::default();
    for edit in edits {
        if edit.frame_label == LICENSE_URL_FRAME
            || edit.frame_label.eq_ignore_ascii_case(LICENSE_LIST_FRAME)
        {
            for url in split_license_list(&edit.value) {
                if !urls.iter().any(|held| license_urls_equal(held, &url)) {
                    urls.push(url);
                }
            }
        } else {
            resolution.edits.push(edit.clone());
        }
    }
    let frame_label = match urls.len() {
        0 => return resolution,
        1 => LICENSE_URL_FRAME,
        _ => LICENSE_LIST_FRAME,
    };
    resolution.replaces_license = true;
    resolution.edits.push(Id3v24Edit {
        frame_label: frame_label.to_string(),
        value: urls.join(LICENSE_LIST_SEPARATOR),
    });
    resolution
}

/// The plain, lower-cased URL that `edits` is about to write into a `WOAR`
/// or `WOAF` frame. Empty when this write touches neither frame.
fn website_urls_in_edits(edits: &[Id3v24Edit]) -> BTreeSet<String> {
    edits
        .iter()
        .filter(|edit| WEBSITE_FRAME_LABELS.contains(&edit.frame_label.as_str()))
        .map(|edit| embedded_frame_url(&edit.value).to_ascii_lowercase())
        .collect()
}

/// The URL embedded in a `WOAR` or `WOAF` value, with a label an earlier
/// write put before it removed (HitIt ADR 0080 Decision 6). A value with no label
/// is already a plain URL.
fn embedded_frame_url(value: &str) -> &str {
    match value.find("https://").or_else(|| value.find("http://")) {
        Some(start) => value[start..].trim(),
        None => value.trim(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<String>,
    pub total_tracks: Option<String>,
    pub date: Option<String>,
    pub custom: BTreeMap<String, String>,
    pub artwork: Option<EmbeddedArtwork>,
    pub fields: Vec<Id3Field>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedArtwork {
    pub mime_type: String,
    pub picture_type: String,
    pub description: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Id3Field {
    pub frame_id: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Id3v24Edit {
    pub frame_label: String,
    pub value: String,
}

/// Reads the tags of `path`: with the ID3v2 frame loop for MP3, with `lofty`
/// for other formats (HitIt ADR 0004). An MP3 file gives each good frame
/// (musicindex ADR 0093 R-TAG-7).
///
/// # Errors
///
/// Returns an error when the file cannot be read or its tag cannot be parsed.
pub fn read_audio_tags(path: &Path) -> Result<AudioTags> {
    read_audio_tags_checked(path).map(|(tags, _)| tags)
}

/// Reads the tags of `path` as [`read_audio_tags`] does, and also gives the
/// problems of the ID3v2 tag (musicindex ADR 0093 R-TAG-7 item 2). A format
/// other than MP3 gives no problem.
///
/// # Errors
///
/// Returns an error when the file cannot be read or its tag cannot be parsed.
pub fn read_audio_tags_checked(path: &Path) -> Result<(AudioTags, TagProblems)> {
    match AudioFormat::detect_from_file(path) {
        // Unknown format — fall back to id3 so we don't regress on files whose
        // magic bytes weren't recognised (e.g. unusual MP3 headers).
        Ok(AudioFormat::Mp3) | Err(_) => read_mp3_tags(path),
        Ok(_) => Ok((read_lofty_tags(path)?, TagProblems::default())),
    }
}

fn read_lofty_tags(path: &Path) -> Result<AudioTags> {
    use lofty::file::TaggedFileExt;
    use lofty::prelude::{Accessor, ItemKey};
    use lofty::probe::Probe;
    use lofty::tag::{ItemValue, Tag as LoftyTag};

    fn cow_to_string(value: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
        value.map(std::borrow::Cow::into_owned)
    }

    let tagged = Probe::open(path)
        .with_context(|| format!("probe {}", path.display()))?
        .read()
        .with_context(|| format!("read tags from {}", path.display()))?;

    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(AudioTags::default());
    };

    let title = cow_to_string(Accessor::title(tag));
    let artist = cow_to_string(Accessor::artist(tag));
    let album = cow_to_string(Accessor::album(tag));
    let track_number = Accessor::track(tag).map(|n| n.to_string());
    let total_tracks = Accessor::track_total(tag).map(|n| n.to_string());
    let date = LoftyTag::get_string(tag, &ItemKey::RecordingDate)
        .map(std::string::ToString::to_string)
        .or_else(|| Accessor::year(tag).map(|y| y.to_string()));

    let mut custom = BTreeMap::new();
    let mut fields: Vec<Id3Field> = Vec::new();
    for item in tag.items() {
        let key_label = lofty_item_label(item.key());
        let value = match item.value() {
            ItemValue::Text(text) | ItemValue::Locator(text) => text.clone(),
            ItemValue::Binary(_) => continue,
        };
        if let ItemKey::Unknown(name) = item.key() {
            custom.insert(name.clone(), value.clone());
        }
        fields.push(Id3Field {
            frame_id: key_label,
            value,
        });
    }

    let artwork = tag.pictures().first().map(|pic| EmbeddedArtwork {
        mime_type: pic.mime_type().map_or_else(
            || "application/octet-stream".into(),
            std::string::ToString::to_string,
        ),
        picture_type: format!("{:?}", pic.pic_type()),
        description: pic.description().unwrap_or("").to_string(),
        data: pic.data().to_vec(),
    });

    add_lofty_compare_aliases(&mut fields, artwork.as_ref());

    Ok(AudioTags {
        title,
        artist,
        album,
        track_number,
        total_tracks,
        date,
        custom,
        artwork,
        fields,
    })
}

fn add_lofty_compare_aliases(fields: &mut Vec<Id3Field>, artwork: Option<&EmbeddedArtwork>) {
    if let Some(comment) = first_field_value(fields, "COMM") {
        push_alias_field(fields, "COMM:Podcast Description", comment);
    }
    add_lofty_album_description_alias(fields);

    if let Some(transcript) =
        first_field_value(fields, "USLT").or_else(|| first_field_value(fields, "SYLT"))
    {
        push_alias_field(fields, "USLT:Podcast Transcript", transcript.clone());
        push_alias_field(fields, "SYLT:Podcast Transcript", transcript);
    }

    if let Some(artwork) = artwork {
        let summary = if artwork.description.trim().is_empty() {
            format!(
                "{} ({}, {} bytes)",
                artwork.picture_type,
                artwork.mime_type,
                artwork.data.len()
            )
        } else {
            format!(
                "{}: {} ({}, {} bytes)",
                artwork.description,
                artwork.picture_type,
                artwork.mime_type,
                artwork.data.len()
            )
        };
        push_alias_field(fields, "APIC", summary);
    }
}

/// HitIt ADR 0080 Decisions 3 and 5: on Vorbis Comments and MP4 freeform atoms,
/// the album description round-trips as a `TXXX`-style field, the same way
/// any other named `TXXX` descriptor does (`lofty_field_for_label` gives it
/// that key when the writer writes it). This aliases the field back to
/// `COMM:Podcast Feed Description`, so the compare grid and the tag
/// scan find it under the frame the writer gives it.
fn add_lofty_album_description_alias(fields: &mut Vec<Id3Field>) {
    let Some(value) = fields.iter().find_map(|field| {
        let descriptor = field.frame_id.strip_prefix("TXXX:")?;
        descriptor
            .eq_ignore_ascii_case("Podcast Feed Description")
            .then(|| field.value.clone())
    }) else {
        return;
    };
    push_alias_field(fields, "COMM:Podcast Feed Description", value);
}

fn push_alias_field(fields: &mut Vec<Id3Field>, frame_id: &str, value: String) {
    if fields.iter().any(|field| field.frame_id == frame_id) {
        return;
    }
    fields.push(Id3Field {
        frame_id: frame_id.to_string(),
        value,
    });
}

fn first_field_value(fields: &[Id3Field], frame_base: &str) -> Option<String> {
    fields.iter().find_map(|field| {
        field
            .frame_id
            .split(':')
            .next()
            .is_some_and(|base| base == frame_base)
            .then(|| field.value.clone())
    })
}

/// Map a lofty `ItemKey` (Vorbis Comment / MP4 atom / etc.) onto the
/// equivalent ID3v2.4 frame label so the metadata comparator — which speaks
/// ID3 frame IDs natively — can match values regardless of source container.
///
/// Without this, FLAC/OGG/MP4 reads emit Debug strings like `"TrackTitle"`
/// that no comparator row recognises, making every populated field look
/// like an unapplied pending edit.
fn lofty_item_label(key: &lofty::prelude::ItemKey) -> String {
    use crate::field::TagFieldId;
    use lofty::prelude::ItemKey;
    use lofty::tag::TagType;
    match key {
        // HitIt ADR 0080 Decision 11: lofty reads the `LICENSE` key of a Vorbis
        // comment and of an MP4 freeform atom as its own item key. The
        // compare grid and the scan know it as `TXXX:LICENSE`.
        ItemKey::License => "TXXX:LICENSE".into(),
        ItemKey::Unknown(name) => TagFieldId::from_storage_key_name(name).map_or_else(
            || format!("TXXX:{name}"),
            |field| match field {
                TagFieldId::Custom(desc) => format!("TXXX:{desc}"),
                TagFieldId::Url(kind) => kind.to_id3().to_string(),
                TagFieldId::Title => "TIT2".into(),
                TagFieldId::Artist => "TPE1".into(),
                TagFieldId::AlbumArtist => "TPE2".into(),
                TagFieldId::Album => "TALB".into(),
                TagFieldId::TrackNumber | TagFieldId::TotalTracks => "TRCK".into(),
                TagFieldId::DiscNumber => "TPOS".into(),
                TagFieldId::Date => "TDRC".into(),
                TagFieldId::Composer => "TCOM".into(),
                TagFieldId::Genre => "TCON".into(),
                TagFieldId::Publisher => "TPUB".into(),
                TagFieldId::Language => "TLAN".into(),
                TagFieldId::Isrc => "TSRC".into(),
                TagFieldId::Comment => "COMM".into(),
                TagFieldId::Lyrics => "USLT".into(),
                TagFieldId::Id3Text(label)
                | TagFieldId::Id3Url(label)
                | TagFieldId::Id3Raw(label) => label,
            },
        ),
        other => other
            .map_key(TagType::Id3v2, false)
            .map_or_else(|| format!("{other:?}"), std::string::ToString::to_string),
    }
}

/// The result of one tag write (HitIt ADR 0080 task 004).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TagWriteResult {
    /// The number of edits that the write applied.
    pub applied: usize,
    /// The ID of each old frame that the write removed, because an ID3v2.4
    /// tag cannot hold its ID.
    pub removed_frames: Vec<String>,
    /// The label of each old frame of musicindex ADR 0093 R-TAG-2 item 5 that
    /// the write removed, for example `TXXX:MusicIndex Feed Guid`. A Vorbis
    /// comment or an MP4 atom gives its stored key.
    pub removed_old_frames: Vec<String>,
    /// Each edit that the write did not apply, with the reason. A picture
    /// that the reference source cannot give is an example.
    pub skipped: Vec<String>,
    /// Each bad frame that the write removed (musicindex ADR 0093 R-TAG-9).
    pub bad_frames: Vec<BadFrame>,
    /// True when the write removed an ID3v1 tag (musicindex ADR 0093 R-TAG-8
    /// item 5).
    pub removed_id3v1: bool,
}

/// Writes `edits` into the tag of `path` (HitIt ADR 0008). The function uses
/// `id3` for MP3 and `lofty` for FLAC, Ogg and MP4. `source` gives the bytes
/// of a picture and the text of a transcript for an `APIC`, `USLT` or `SYLT`
/// edit.
///
/// Each write also removes the old frames of musicindex ADR 0093 R-TAG-2
/// item 5. A second write of the same edits gives the same tag. An empty
/// `edits` removes only the old frames. When the file has no old frame, an
/// empty `edits` does not write the file.
///
/// # Errors
///
/// Returns an error when the file cannot be read or written, when an edit
/// label cannot be written, or when the format is raw WAV and `edits` is not
/// empty. A failed MP3 write
/// does not change the file.
pub fn write_id3v24_edits(
    path: &Path,
    edits: &[Id3v24Edit],
    source: &dyn ReferenceSource,
) -> Result<TagWriteResult> {
    write_tags(path, edits, None, source)
}

/// The writer of [`write_id3v24_edits`] and of the episode writer.
/// `chapters` is `None` when the write does not change the chapter frames.
pub(crate) fn write_tags(
    path: &Path,
    edits: &[Id3v24Edit],
    chapters: Option<&ChapterFrames>,
    source: &dyn ReferenceSource,
) -> Result<TagWriteResult> {
    let (edits, skipped) = without_length_edits(edits);
    let mut result = write_format_tags(path, &edits, chapters, source)?;
    result.skipped.extend(skipped);
    Ok(result)
}

/// Musicindex ADR 0093 R-TAG-11 items 1 and 2: a write does not use the
/// value of a `TLEN` edit. It gives the edits without `TLEN`, and the reason
/// for each `TLEN` edit for [`TagWriteResult::skipped`]. Vorbis comments and
/// MP4 have no length field, so the rule also drops the edit there.
fn without_length_edits(edits: &[Id3v24Edit]) -> (Vec<Id3v24Edit>, Vec<String>) {
    let (length, kept): (Vec<_>, Vec<_>) = edits
        .iter()
        .cloned()
        .partition(|edit| edit.frame_label == "TLEN");
    let skipped = length
        .iter()
        .map(|edit| {
            format!(
                "TLEN {}: a write keeps the TLEN of the file or measures the length \
                 (musicindex ADR 0093 R-TAG-11)",
                edit.value
            )
        })
        .collect();
    (kept, skipped)
}

/// The writer of [`write_tags`] for the format of `path`.
fn write_format_tags(
    path: &Path,
    edits: &[Id3v24Edit],
    chapters: Option<&ChapterFrames>,
    source: &dyn ReferenceSource,
) -> Result<TagWriteResult> {
    match AudioFormat::detect_from_file(path) {
        Ok(AudioFormat::Mp3) | Err(_) => write_mp3_edits(path, edits, chapters, source),
        Ok(AudioFormat::Flac | AudioFormat::OggVorbis | AudioFormat::OggOpus) => write_lofty_edits(
            path,
            edits,
            chapters,
            lofty::tag::TagType::VorbisComments,
            source,
        ),
        Ok(AudioFormat::Mp4) => {
            let edits = single_date_edits(edits);
            write_lofty_edits(path, &edits, chapters, lofty::tag::TagType::Mp4Ilst, source)
        }
        Ok(AudioFormat::Wav) if edits.is_empty() && chapters.is_none() => {
            Ok(TagWriteResult::default())
        }
        Ok(AudioFormat::Wav) => Err(anyhow!(
            "cannot tag raw WAV ({}); convert the file to FLAC first",
            path.display()
        )),
    }
}

/// HitIt ADR 0080 Decision 6: an MP4 file holds one date atom. The `TDRC` edit
/// gives the full date. The `TYER` edit gives its year. The reader gives
/// both atoms as `TDRC`, so two atoms could not be told apart. This keeps
/// the `TDRC` edit, drops the `TYER` edit when a `TDRC` edit exists, and
/// stores a lone `TYER` edit under the `TDRC` atom.
///
/// An MP3 write uses the same edits, because an ID3v2.4 tag has no `TYER`
/// frame (musicindex ADR 0093 R-TAG-8 item 1).
fn single_date_edits(edits: &[Id3v24Edit]) -> Vec<Id3v24Edit> {
    let has_full_date = edits.iter().any(|edit| edit.frame_label == "TDRC");
    edits
        .iter()
        .filter(|edit| !(has_full_date && edit.frame_label == "TYER"))
        .map(|edit| {
            if edit.frame_label == "TYER" {
                Id3v24Edit {
                    frame_label: "TDRC".into(),
                    value: edit.value.clone(),
                }
            } else {
                edit.clone()
            }
        })
        .collect()
}

fn write_mp3_edits(
    path: &Path,
    edits: &[Id3v24Edit],
    chapters: Option<&ChapterFrames>,
    source: &dyn ReferenceSource,
) -> Result<TagWriteResult> {
    let read = read_id3v2(path)?;
    if let Some(cause) = read.problems.bad_tag {
        return Err(BadTag { cause }.into());
    }
    let mut tag = read.tag;
    let old_frames = convert_old_frames(&mut tag, read.version);
    let removed_old_frames = remove_old_id3_frames(&mut tag);
    let repairs = Repairs {
        bad_frames: !read.problems.bad_frames.is_empty() || read.plain_frame_sizes,
        old_frames: !old_frames.is_empty() || !removed_old_frames.is_empty(),
        old_version: read.version.is_some_and(|major| major != 4),
        id3v1: read.has_id3v1,
    };
    if is_empty_write(edits, chapters, repairs) {
        return Ok(TagWriteResult::default());
    }
    let removed_frames = old_frames.removed;
    let edits = single_date_edits(edits);
    let edits = edits.as_slice();
    remove_stale_id3_website_frames(&mut tag, edits);
    remove_stale_id3_description_frames(&mut tag, edits);
    let license = resolve_id3_license_edits(&mut tag, edits);
    let edits = license.edits.as_slice();
    let mut applied = 0;

    for edit in edits {
        let frame = id3v24_edit_frame(edit, source)?;
        tag.add_frame(frame);
        applied += 1;
    }
    if let Some(chapters) = chapters {
        chapters.replace_id3_frames(&mut tag);
    }

    // Musicindex ADR 0093 R-TAG-11: the frame loop removed a TLEN that is
    // not a positive whole number, so a TLEN here is good and stays. The
    // TLEN frame is always the last frame, so a second write of the same
    // tags gives the same bytes.
    let length = tag.remove("TLEN").into_iter().next().or_else(|| {
        crate::track::measured_length(path)
            .map(|length| Frame::text("TLEN", length.as_millis().to_string()))
    });
    if let Some(length) = length {
        tag.add_frame(length);
    }

    write_id3_tag_safely(path, &tag, read.tag_len, read.has_id3v1)?;
    Ok(TagWriteResult {
        applied,
        removed_frames,
        removed_old_frames,
        skipped: Vec::new(),
        bad_frames: read.problems.bad_frames,
        removed_id3v1: read.has_id3v1,
    })
}

/// The changes of a write that are not edits. Musicindex ADR 0093 R-TAG-8
/// item 4: a bad frame, an old frame, an older tag version and an ID3v1 tag
/// are each a change. An old frame is a frame of an older version or a
/// frame of ADR 0093 R-TAG-2 item 5. A plain ID3v2.4 frame size counts as a
/// bad frame (R-TAG-7 item 4).
#[derive(Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)]
struct Repairs {
    bad_frames: bool,
    old_frames: bool,
    old_version: bool,
    id3v1: bool,
}

/// True when a write has no edit, no chapters and no repair. Then the file
/// does not change, so the writer does not write it.
fn is_empty_write(
    edits: &[Id3v24Edit],
    chapters: Option<&ChapterFrames>,
    repairs: Repairs,
) -> bool {
    let Repairs {
        bad_frames,
        old_frames,
        old_version,
        id3v1,
    } = repairs;
    edits.is_empty() && chapters.is_none() && !(bad_frames || old_frames || old_version || id3v1)
}

/// Musicindex ADR 0093 R-TAG-2 item 5: removes each `TXXX`, `COMM`, `USLT`
/// and `SYLT` frame with a description that starts with `MusicIndex `. It
/// returns the label of each removed frame.
fn remove_old_id3_frames(tag: &mut Tag) -> Vec<String> {
    let mut removed = Vec::new();
    tag.frames_vec_mut().retain(|frame| {
        let description = match frame.content() {
            Content::ExtendedText(text) => text.description.as_str(),
            Content::Comment(comment) => comment.description.as_str(),
            Content::Lyrics(lyrics) => lyrics.description.as_str(),
            Content::SynchronisedLyrics(lyrics) => lyrics.description.as_str(),
            _ => return true,
        };
        if frames::is_old_frame(frame.id(), description) {
            removed.push(format!("{}:{description}", frame.id()));
            false
        } else {
            true
        }
    });
    removed
}

/// HitIt ADR 0080 Decision 11, for an MP3 file. This function removes each `WCOP`
/// and `TXXX:LICENSE` value when the write holds a license edit. It returns
/// the edits to apply.
fn resolve_id3_license_edits(tag: &mut Tag, edits: &[Id3v24Edit]) -> LicenseResolution {
    let resolution = resolve_license_edits(edits);
    if resolution.replaces_license {
        tag.frames_vec_mut().retain(|frame| {
            let is_list_frame = frame.id() == "TXXX"
                && frame
                    .content()
                    .extended_text()
                    .is_some_and(|text| text.description.eq_ignore_ascii_case("LICENSE"));
            frame.id() != LICENSE_URL_FRAME && !is_list_frame
        });
    }
    resolution
}

/// What a write does with a frame of ID3v2.2 or ID3v2.3 (musicindex ADR 0093
/// R-TAG-8 item 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OldFrame {
    /// The frame gets the ID3v2.4 ID and keeps its content.
    Rename(&'static str),
    /// `TYER`, `TDAT` and `TIME` merge into one `TDRC`.
    MergeIntoTdrc,
    /// The frame has no sure ID3v2.4 equivalent, so the write removes it
    /// (R-TAG-8 item 3).
    Remove,
}

/// The frames of ID3v2.2 and ID3v2.3 with no ID3v2.4 ID. The `id3` crate
/// gives an ID3v2.2 frame its ID3v2.3 ID when it knows one, so this table
/// has the ID3v2.2 IDs from iTunes that the `id3` table does not convert
/// (HitIt ADR 0080 task 004) and the ID3v2.3 IDs.
const OLD_FRAME_IDS: [(&str, OldFrame); 15] = [
    ("TSP", OldFrame::Rename("TSOP")),
    ("TSA", OldFrame::Rename("TSOA")),
    ("TST", OldFrame::Rename("TSOT")),
    ("TS2", OldFrame::Rename("TSO2")),
    ("TSC", OldFrame::Rename("TSOC")),
    ("TCP", OldFrame::Rename("TCMP")),
    ("TORY", OldFrame::Rename("TDOR")),
    ("IPLS", OldFrame::Rename("TIPL")),
    ("TYER", OldFrame::MergeIntoTdrc),
    ("TDAT", OldFrame::MergeIntoTdrc),
    ("TIME", OldFrame::MergeIntoTdrc),
    ("TRDA", OldFrame::Remove),
    ("TSIZ", OldFrame::Remove),
    ("EQUA", OldFrame::Remove),
    ("RVAD", OldFrame::Remove),
];

/// Musicindex ADR 0093 R-TAG-8 items 2 and 3: gives each old frame its
/// ID3v2.4 equivalent, or removes it. A frame of ID3v2.2 whose ID is not 4
/// bytes long has no equivalent. The `id3` crate gives the ID3v2.2 `RVA`
/// frame the ID `RVA2`, but the content of `RVA2` is different, so the write
/// removes it. `version` is the major version of the tag that the file held.
pub(crate) fn convert_old_frames(tag: &mut Tag, version: Option<u8>) -> OldFrameChanges {
    let mut removed = Vec::new();
    let mut converted_ids = Vec::new();
    let mut converted = Vec::new();
    let mut date = OldDate::default();
    tag.frames_vec_mut().retain(|frame| {
        let id = frame.id();
        if version == Some(2) && id == "RVA2" {
            removed.push(id.to_owned());
            return false;
        }
        let row = OLD_FRAME_IDS.iter().find(|(old_id, _)| *old_id == id);
        match row {
            Some((_, OldFrame::Rename(new_id))) => {
                converted_ids.push(id.to_owned());
                converted.push(Frame::with_content(*new_id, frame.content().clone()));
            }
            Some((_, OldFrame::MergeIntoTdrc)) => {
                date.set(id, frame.content().text());
            }
            None if id.len() == 4 => return true,
            Some((_, OldFrame::Remove)) | None => removed.push(id.to_owned()),
        }
        false
    });
    for frame in converted {
        tag.add_frame(frame);
    }
    match date.tdrc() {
        Some((tdrc, used)) if tag.get("TDRC").is_none() => {
            tag.add_frame(Frame::text("TDRC", tdrc));
            for id in date.ids() {
                if used.contains(&id.as_str()) {
                    converted_ids.push(id);
                } else {
                    removed.push(id);
                }
            }
        }
        _ => removed.extend(date.ids()),
    }
    OldFrameChanges {
        removed,
        converted: converted_ids,
    }
}

/// The old frames that [`convert_old_frames`] changed.
pub(crate) struct OldFrameChanges {
    /// The ID of each frame that the conversion removed.
    pub(crate) removed: Vec<String>,
    /// The ID of each frame that the conversion changed to its ID3v2.4
    /// equivalent.
    pub(crate) converted: Vec<String>,
}

impl OldFrameChanges {
    /// True when the conversion changed no frame.
    pub(crate) fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.converted.is_empty()
    }
}

/// The values of `TYER`, `TDAT` and `TIME` in one tag.
#[derive(Default)]
struct OldDate {
    year: Option<String>,
    day_month: Option<String>,
    time: Option<String>,
}

impl OldDate {
    fn set(&mut self, id: &str, text: Option<&str>) {
        let value = text.map(|text| text.trim().to_owned());
        match id {
            "TYER" => self.year = value,
            "TDAT" => self.day_month = value,
            _ => self.time = value,
        }
    }

    fn ids(&self) -> Vec<String> {
        [
            ("TYER", &self.year),
            ("TDAT", &self.day_month),
            ("TIME", &self.time),
        ]
        .into_iter()
        .filter(|(_, value)| value.is_some())
        .map(|(id, _)| id.to_owned())
        .collect()
    }

    /// The `TDRC` text: `yyyy`, `yyyy-MM-dd` or `yyyy-MM-ddTHH:mm`, and the
    /// IDs of the frames that it uses. `TDAT` is `DDMM` and `TIME` is `HHMM`.
    /// A part that is not 4 digits or not a valid date or time stops the
    /// merge there, because its reading is not sure (R-TAG-9 item 1). `None`
    /// when the year is not 4 digits.
    fn tdrc(&self) -> Option<(String, Vec<&'static str>)> {
        let digits = |value: &Option<String>| {
            value
                .as_deref()
                .filter(|value| value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_digit()))
                .map(|value| (value[..2].to_owned(), value[2..].to_owned()))
        };
        let in_range = |part: &str, low: u8, high: u8| {
            part.parse::<u8>()
                .is_ok_and(|number| (low..=high).contains(&number))
        };
        let year = self
            .year
            .as_deref()
            .filter(|year| year.len() == 4 && year.bytes().all(|byte| byte.is_ascii_digit()))?;
        let mut tdrc = year.to_owned();
        let mut used = vec!["TYER"];
        let Some((day, month)) = digits(&self.day_month)
            .filter(|(day, month)| in_range(day, 1, 31) && in_range(month, 1, 12))
        else {
            return Some((tdrc, used));
        };
        tdrc.extend(["-", &month, "-", &day]);
        used.push("TDAT");
        if let Some((hour, minute)) = digits(&self.time)
            .filter(|(hour, minute)| in_range(hour, 0, 23) && in_range(minute, 0, 59))
        {
            tdrc.extend(["T", &hour, ":", &minute]);
            used.push("TIME");
        }
        Some((tdrc, used))
    }
}

/// HitIt ADR 0080 task 004: a failed write never changes the file. The writer
/// encodes the tag into memory first. It then writes the new tag and the
/// bytes after the old tag into a staged copy in the same directory, and
/// renames the copy over the file. The copy keeps the file mode.
///
/// `old_tag_len` is the length of the old ID3v2 tag from the frame loop,
/// with its extended header and its footer (musicindex ADR 0093 R-TAG-7).
/// The writer does not find the end of the old tag again, so the bytes after
/// the tag stay equal to the bytes of the original file. When `remove_id3v1`
/// is true, the copy does not get the last 128 bytes, which are the ID3v1
/// tag (R-TAG-8 item 5).
fn write_id3_tag_safely(
    path: &Path,
    tag: &Tag,
    old_tag_len: u64,
    remove_id3v1: bool,
) -> Result<()> {
    let mut bytes = Vec::new();
    tag.write_to(&mut bytes, Version::Id3v24)
        .with_context(|| format!("encode ID3v2.4 tags for {}", path.display()))?;
    let target = fs::canonicalize(path)
        .with_context(|| format!("resolve the tag write target {}", path.display()))?;
    let original = fs::read(&target).with_context(|| format!("read {}", target.display()))?;
    let start = usize::try_from(old_tag_len)
        .ok()
        .filter(|start| *start <= original.len())
        .with_context(|| format!("the old tag of {} is past its end", target.display()))?;
    let end = if remove_id3v1 && original.len() - start >= ID3V1_LEN {
        original.len() - ID3V1_LEN
    } else {
        original.len()
    };
    bytes.extend_from_slice(&original[start..end]);
    let permissions = fs::metadata(&target)
        .with_context(|| format!("read the mode of {}", target.display()))?
        .permissions();

    let staged = staged_tag_path(&target)?;
    let written = fs::write(&staged, &bytes)
        .with_context(|| format!("write {}", staged.display()))
        .and_then(|()| {
            fs::set_permissions(&staged, permissions)
                .with_context(|| format!("set the mode of {}", staged.display()))
        })
        .and_then(|()| {
            fs::rename(&staged, &target)
                .with_context(|| format!("replace {} with {}", target.display(), staged.display()))
        });
    if written.is_err() {
        let _ = fs::remove_file(&staged);
    }
    written.with_context(|| format!("write ID3v2.4 tags to {}", path.display()))
}

/// The length of an ID3v1 tag at the end of a file.
const ID3V1_LEN: usize = 128;

/// A path for the staged copy of `target`, in the directory of `target`.
fn staged_tag_path(target: &Path) -> Result<std::path::PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let directory = target
        .parent()
        .with_context(|| format!("find the directory of {}", target.display()))?;
    let name = target
        .file_name()
        .with_context(|| format!("find the file name of {}", target.display()))?;
    let mut staged_name = std::ffi::OsString::from(".");
    staged_name.push(name);
    staged_name.push(format!(
        ".hitit-tag-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    Ok(directory.join(staged_name))
}

/// HitIt ADR 0080 Decision 6: a write removes a `WOAR` or `WOAF` value under
/// either label when the same URL is about to be written again. This
/// recognizes the earlier labeled channel value and the item page
/// mistakenly held in `WOAR`, and removes only those. A value from another
/// tool, or a `MusicBrainz` value this write does not repeat, is not touched.
fn remove_stale_id3_website_frames(tag: &mut Tag, edits: &[Id3v24Edit]) {
    let new_urls = website_urls_in_edits(edits);
    if new_urls.is_empty() {
        return;
    }
    tag.frames_vec_mut().retain(|frame| {
        if !WEBSITE_FRAME_LABELS.contains(&frame.id()) {
            return true;
        }
        // HitIt ADR 0080 Decision 6: the values of this write replace each value
        // of a frame that it supplies.
        if edits.iter().any(|edit| edit.frame_label == frame.id()) {
            return false;
        }
        match frame.content().link() {
            Some(link) => !new_urls.contains(&embedded_frame_url(link).to_ascii_lowercase()),
            None => true,
        }
    });
}

/// HitIt ADR 0080 Decisions 5 and 6: `COMM:Podcast Description` and
/// `COMM:Podcast Feed Description` hold at most one value between them.
/// When this write sets either one, it clears both first, so a fact that
/// moved descriptor leaves no stale copy under its earlier one.
fn remove_stale_id3_description_frames(tag: &mut Tag, edits: &[Id3v24Edit]) {
    let touches_description = edits
        .iter()
        .any(|edit| DESCRIPTION_FRAME_LABELS.contains(&edit.frame_label.as_str()));
    if !touches_description {
        return;
    }
    tag.frames_vec_mut().retain(|frame| {
        let Content::Comment(comment) = frame.content() else {
            return true;
        };
        let label = format!("COMM:{}", comment.description);
        !DESCRIPTION_FRAME_LABELS.contains(&label.as_str())
    });
}

#[allow(clippy::too_many_lines, reason = "one pass over the edits of one tag")]
fn write_lofty_edits(
    path: &Path,
    edits: &[Id3v24Edit],
    chapters: Option<&ChapterFrames>,
    tag_type: lofty::tag::TagType,
    source: &dyn ReferenceSource,
) -> Result<TagWriteResult> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::prelude::{Accessor, ItemKey};
    use lofty::probe::Probe;
    use lofty::tag::{ItemValue, Tag as LoftyTag, TagItem};

    use crate::field::TagFieldId;

    let mut tagged = Probe::open(path)
        .with_context(|| format!("probe {}", path.display()))?
        .read()
        .with_context(|| format!("read tags from {}", path.display()))?;

    let mut tag = tagged
        .remove(tag_type)
        .unwrap_or_else(|| LoftyTag::new(tag_type));
    let removed_old_frames = remove_old_lofty_items(&mut tag, tag_type);
    let repairs = Repairs {
        old_frames: !removed_old_frames.is_empty(),
        ..Repairs::default()
    };
    if is_empty_write(edits, chapters, repairs) {
        return Ok(TagWriteResult::default());
    }
    // Musicindex ADR 0093 R-TAG-1 item 5: no chapter frames in this format.
    // The chapter JSON is a keyed item, so it is one more edit.
    let mut edits = edits.to_vec();
    if let Some(chapters) = chapters {
        match chapters.json() {
            Some(json) => edits.push(Id3v24Edit {
                frame_label: frames::CHAPTERS.to_owned(),
                value: json.to_owned(),
            }),
            None => {
                if let Some(key) = lofty_generic_key(frames::CHAPTERS, tag_type) {
                    remove_lofty_item_by_key(&mut tag, tag_type, &key);
                }
            }
        }
    }
    // HitIt ADR 0080 Decision 11: remove the license values of the file before any
    // cleanup pass reads the edits.
    let license = resolve_lofty_license_edits(&mut tag, tag_type, &edits);
    let edits = license.edits.as_slice();
    // Each cleanup pass runs once, before any edit is added. A pass inside
    // the loop would remove the value of an earlier edit that shares its
    // key with a later one (orchestrator review, defect 2).
    remove_stale_lofty_website_items(&mut tag, tag_type, edits);
    remove_stale_lofty_description_items(&mut tag, tag_type, edits);
    remove_stale_lofty_keyed_items(&mut tag, tag_type, edits);

    let mut applied = 0usize;
    let mut skipped = Vec::new();
    for edit in edits {
        // Artwork lands as a Picture rather than a text item.
        if edit.frame_label.starts_with("APIC") {
            match read_picture_reference(&edit.value, source) {
                Ok((mime, data)) => {
                    tag.push_picture(Picture::new_unchecked(
                        PictureType::CoverFront,
                        Some(MimeType::from_str(&mime)),
                        None,
                        data,
                    ));
                    applied += 1;
                }
                Err(err) => {
                    skipped.push(format!("{}: {err:#}", edit.frame_label));
                }
            }
            continue;
        }

        let field = lofty_field_for_label(&edit.frame_label);
        let inserted = match tag_type {
            lofty::tag::TagType::VorbisComments => {
                let Some(key) = field.vorbis_key() else {
                    continue;
                };
                // `Tag::push` rejects `ItemKey::Unknown` because re_map
                // fails for keys without a built-in mapping, so this uses
                // the unchecked push instead. The cleanup above already
                // removed one item for each key this write owns, so a
                // plain push here does not duplicate on a repeated write,
                // and two edits that share a key (`TDRC` and `TYER`, both
                // `DATE`) both land.
                tag.push_unchecked(TagItem::new(
                    ItemKey::Unknown(key),
                    ItemValue::Text(edit.value.clone()),
                ));
                true
            }
            lofty::tag::TagType::Mp4Ilst => {
                let handled = match &field {
                    TagFieldId::Title => {
                        Accessor::set_title(&mut tag, edit.value.clone());
                        true
                    }
                    TagFieldId::Artist => {
                        Accessor::set_artist(&mut tag, edit.value.clone());
                        true
                    }
                    TagFieldId::Album => {
                        Accessor::set_album(&mut tag, edit.value.clone());
                        true
                    }
                    TagFieldId::TrackNumber => {
                        if let Ok(n) = edit.value.parse::<u32>() {
                            Accessor::set_track(&mut tag, n);
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                if !handled {
                    let ns_key = lofty_mp4_freeform_key(&edit.frame_label, &field);
                    tag.push_unchecked(TagItem::new(
                        ItemKey::Unknown(ns_key),
                        ItemValue::Text(edit.value.clone()),
                    ));
                }
                true
            }
            _ => false,
        };
        if inserted {
            applied += 1;
        }
    }

    tagged.insert_tag(tag);
    tagged
        .save_to_path(path, lofty::config::WriteOptions::default())
        .with_context(|| format!("write tags to {}", path.display()))?;
    Ok(TagWriteResult {
        applied,
        removed_frames: Vec::new(),
        removed_old_frames,
        skipped,
        bad_frames: Vec::new(),
        removed_id3v1: false,
    })
}

/// Musicindex ADR 0093 R-TAG-2 item 5, for Vorbis comments and MP4 freeform
/// atoms: removes each item with a key that starts with `MusicIndex `. An MP4
/// key starts with `----:com.apple.iTunes:`. It returns each removed key.
fn remove_old_lofty_items(tag: &mut lofty::tag::Tag, tag_type: lofty::tag::TagType) -> Vec<String> {
    let mut removed = Vec::new();
    tag.retain(|item| {
        let Some(key) = item.key().map_key(tag_type, true) else {
            return true;
        };
        let name = key.strip_prefix(MP4_FREEFORM_PREFIX).unwrap_or(key);
        if frames::is_old_description(name) {
            removed.push(key.to_owned());
            false
        } else {
            true
        }
    });
    removed
}

/// The start of the key of an MP4 freeform atom that this writer writes.
const MP4_FREEFORM_PREFIX: &str = "----:com.apple.iTunes:";

/// HitIt ADR 0080 Decision 11, for Vorbis Comments and MP4 freeform atoms. This
/// function removes the `WCOP` and `TXXX:LICENSE` keys when the write holds a
/// license edit. It returns the edits to apply.
fn resolve_lofty_license_edits(
    tag: &mut lofty::tag::Tag,
    tag_type: lofty::tag::TagType,
    edits: &[Id3v24Edit],
) -> LicenseResolution {
    let resolution = resolve_license_edits(edits);
    if resolution.replaces_license {
        for label in [LICENSE_URL_FRAME, LICENSE_LIST_FRAME] {
            if let Some(key) = lofty_generic_key(label, tag_type) {
                remove_lofty_item_by_key(tag, tag_type, &key);
            }
        }
    }
    resolution
}

/// HitIt ADR 0080 Decision 5: the field that `frame_label` maps to, for the lofty
/// writer. `COMM:Podcast Feed Description` gets its own key, the same
/// way a `TXXX` descriptor does, so it does not collide with the shared
/// Comment key (Vorbis `COMMENT`, MP4 `©cmt`) that
/// `COMM:Podcast Description` keeps.
fn lofty_field_for_label(frame_label: &str) -> crate::field::TagFieldId {
    use crate::field::TagFieldId;
    if frame_label == "COMM:Podcast Feed Description" {
        TagFieldId::Custom("Podcast Feed Description".into())
    } else {
        TagFieldId::from_id3_label(frame_label)
    }
}

/// The MP4 freeform atom name that `field` stores under, once `field` is
/// known not to be one of the atoms an MP4 accessor sets directly.
fn lofty_mp4_freeform_key(frame_label: &str, field: &crate::field::TagFieldId) -> String {
    use crate::field::TagFieldId;
    match field {
        TagFieldId::Custom(desc) => format!("----:com.apple.iTunes:{desc}"),
        TagFieldId::Url(kind) => format!("----:com.apple.iTunes:{}", kind.to_id3()),
        // HitIt ADR 0080 Decision 13: the language lands under the `LANGUAGE` atom,
        // the key that the Vorbis Comment uses.
        TagFieldId::Language => "----:com.apple.iTunes:LANGUAGE".to_owned(),
        _ => format!("----:com.apple.iTunes:{frame_label}"),
    }
}

/// The key that `frame_label` stores under, in `tag_type`'s tag, through
/// the generic push path. `None` for a Vorbis comment with no mapping for
/// this field, and for a tag type this writer does not handle.
fn lofty_generic_key(frame_label: &str, tag_type: lofty::tag::TagType) -> Option<String> {
    let field = lofty_field_for_label(frame_label);
    match tag_type {
        lofty::tag::TagType::VorbisComments => field.vorbis_key(),
        lofty::tag::TagType::Mp4Ilst => Some(lofty_mp4_freeform_key(frame_label, &field)),
        _ => None,
    }
}

/// HitIt ADR 0080 Decision 6: the Vorbis Comment key that `frame_label` stores
/// under in a FLAC, Ogg Vorbis or Ogg Opus file. The writer uses the same
/// key. Two labels can share one key, for example `TDRC` and `TYER`, both
/// `DATE`. The function gives `None` for other formats and for a label
/// without a Vorbis key.
#[must_use]
pub fn vorbis_storage_key(frame_label: &str, format: crate::format::AudioFormat) -> Option<String> {
    use crate::format::AudioFormat;
    match format {
        AudioFormat::Flac | AudioFormat::OggVorbis | AudioFormat::OggOpus => {
            lofty_generic_key(frame_label, lofty::tag::TagType::VorbisComments)
                .map(|key| key.to_ascii_uppercase())
        }
        AudioFormat::Mp3 | AudioFormat::Mp4 | AudioFormat::Wav => None,
    }
}

/// `true` for the two logical fields that map to `WOAR` and `WOAF`. A
/// website edit is not part of the once-per-key removal that
/// `remove_stale_lofty_keyed_items` does: `WOAR` can hold more than one
/// value (the channel website and a `MusicBrainz` value), and
/// `remove_stale_lofty_website_items` already covers the value this write
/// repeats.
fn is_website_field(field: &crate::field::TagFieldId) -> bool {
    use crate::field::{TagFieldId, UrlKind};
    matches!(
        field,
        TagFieldId::Url(UrlKind::OfficialArtist | UrlKind::OfficialAudio)
    )
}

/// Removes each existing item whose stored key, under `tag_type`, matches
/// `key`. Lofty can read a standard key (`TITLE`, for example) back as its
/// own typed `ItemKey` rather than as `ItemKey::Unknown`, so comparing the
/// mapped key string, instead of the `ItemKey` value, keeps one item for
/// that key across a repeated write (HitIt ADR 0080 Decision 6).
///
/// Lofty reads two stored keys as one typed `ItemKey` for some fields. For
/// example, `ORGANIZATION` and `LABEL` both read as `ItemKey::Label`, and the
/// mapped key of that item is `LABEL`. Thus the function also removes an item
/// whose typed key equals the typed key of `key` (HitIt ADR 0080 Decision 12: a
/// repeated `TPUB` write keeps one value in a Vorbis Comment).
fn remove_lofty_item_by_key(tag: &mut lofty::tag::Tag, tag_type: lofty::tag::TagType, key: &str) {
    use lofty::tag::ItemKey;

    let typed_key = match ItemKey::from_key(tag_type, &key.to_ascii_uppercase()) {
        ItemKey::Unknown(_) => None,
        typed => Some(typed),
    };
    tag.retain(|item| {
        let same_mapped_key = item
            .key()
            .map_key(tag_type, true)
            .is_some_and(|existing| existing.eq_ignore_ascii_case(key));
        let same_typed_key = typed_key.as_ref() == Some(item.key());
        !(same_mapped_key || same_typed_key)
    });
}

/// HitIt ADR 0080 Decision 6: removes each key that `edits` gives a non-website,
/// non-artwork field, once, before any edit of this write is added.
///
/// A pass inside the write loop would remove one edit's value when a later
/// edit that shares its key was added (orchestrator review, defect 2): two
/// edits can share a key, for example `TDRC` and `TYER`, both `DATE` on
/// Vorbis Comments. Clearing each owned key once, first, keeps both edits'
/// values, and still keeps a repeated write from duplicating either one.
fn remove_stale_lofty_keyed_items(
    tag: &mut lofty::tag::Tag,
    tag_type: lofty::tag::TagType,
    edits: &[Id3v24Edit],
) {
    use crate::field::TagFieldId;

    let mut keys = BTreeSet::new();
    for edit in edits {
        if edit.frame_label.starts_with("APIC") {
            continue;
        }
        let field = lofty_field_for_label(&edit.frame_label);
        if is_website_field(&field) {
            continue;
        }
        // An MP4 accessor sets this field directly; it is not a keyed push.
        if tag_type == lofty::tag::TagType::Mp4Ilst
            && matches!(
                field,
                TagFieldId::Title
                    | TagFieldId::Artist
                    | TagFieldId::Album
                    | TagFieldId::TrackNumber
            )
        {
            continue;
        }
        if let Some(key) = lofty_generic_key(&edit.frame_label, tag_type) {
            keys.insert(key);
        }
    }
    // An earlier write stored a second MP4 date atom under `TYER`. A date
    // write removes it, so the file keeps one date atom.
    if tag_type == lofty::tag::TagType::Mp4Ilst
        && let Some(date_key) = lofty_generic_key("TDRC", tag_type)
        && keys.contains(&date_key)
        && let Some(year_key) = lofty_generic_key("TYER", tag_type)
    {
        keys.insert(year_key);
    }
    for key in keys {
        remove_lofty_item_by_key(tag, tag_type, &key);
    }
}

/// HitIt ADR 0080 Decisions 3 and 5, for Vorbis Comments and MP4 freeform atoms:
/// the item's own description and the channel's hold at most one value
/// between them. `COMM:Podcast Feed Description` keeps its own key
/// apart from the shared Comment key that `COMM:Podcast Description`
/// keeps (`lofty_field_for_label`), so the two no longer collide, and a
/// file can now carry the channel's description on these formats. When
/// this write sets either label, it clears the other's stored key first,
/// so a fact that moved owner leaves no stale copy under its earlier key.
fn remove_stale_lofty_description_items(
    tag: &mut lofty::tag::Tag,
    tag_type: lofty::tag::TagType,
    edits: &[Id3v24Edit],
) {
    let touches_description = edits
        .iter()
        .any(|edit| DESCRIPTION_FRAME_LABELS.contains(&edit.frame_label.as_str()));
    if !touches_description {
        return;
    }
    for label in DESCRIPTION_FRAME_LABELS {
        if let Some(key) = lofty_generic_key(label, tag_type) {
            remove_lofty_item_by_key(tag, tag_type, &key);
        }
    }
}

/// HitIt ADR 0080 Decision 6: removes a Vorbis comment or MP4 freeform item under
/// the stored key of `WOAR` or `WOAF` whose URL is about to be written
/// again, so a value that moved frame, or a repeated write, leaves no stale
/// item behind. An item under another key, or a website item with a
/// different URL, is not touched.
fn remove_stale_lofty_website_items(
    tag: &mut lofty::tag::Tag,
    tag_type: lofty::tag::TagType,
    edits: &[Id3v24Edit],
) {
    use lofty::prelude::ItemKey;
    use lofty::tag::ItemValue;

    let new_urls = website_urls_in_edits(edits);
    if new_urls.is_empty() {
        return;
    }
    let keys = lofty_website_item_keys(tag_type);
    tag.retain(|item| {
        let ItemKey::Unknown(name) = item.key() else {
            return true;
        };
        let Some(position) = keys.iter().position(|key| key.eq_ignore_ascii_case(name)) else {
            return true;
        };
        // HitIt ADR 0080 Decision 6: the values of this write replace each value
        // of a frame that it supplies. `lofty_website_item_keys` gives the
        // keys in the sequence of `WEBSITE_FRAME_LABELS`.
        if edits
            .iter()
            .any(|edit| edit.frame_label == WEBSITE_FRAME_LABELS[position])
        {
            return false;
        }
        match item.value() {
            ItemValue::Text(text) => {
                !new_urls.contains(&embedded_frame_url(text).to_ascii_lowercase())
            }
            _ => true,
        }
    });
}

/// The stored item key that holds `WOAR` and `WOAF` in `tag_type`.
fn lofty_website_item_keys(tag_type: lofty::tag::TagType) -> [String; 2] {
    use crate::field::{TagFieldId, UrlKind};

    match tag_type {
        lofty::tag::TagType::VorbisComments => [
            TagFieldId::Url(UrlKind::OfficialArtist)
                .vorbis_key()
                .unwrap_or_default(),
            TagFieldId::Url(UrlKind::OfficialAudio)
                .vorbis_key()
                .unwrap_or_default(),
        ],
        _ => [
            format!("----:com.apple.iTunes:{}", UrlKind::OfficialArtist.to_id3()),
            format!("----:com.apple.iTunes:{}", UrlKind::OfficialAudio.to_id3()),
        ],
    }
}

fn read_mp3_tags(path: &Path) -> Result<(AudioTags, TagProblems)> {
    let read = read_id3v2(path)?;
    Ok((audio_tags_from_id3(&read.tag), read.problems))
}

pub(crate) fn audio_tags_from_id3(tag: &Tag) -> AudioTags {
    AudioTags {
        title: tag.title().map(ToOwned::to_owned),
        artist: tag.artist().map(ToOwned::to_owned),
        album: tag.album().map(ToOwned::to_owned),
        track_number: tag
            .track()
            .map(|number| number.to_string())
            .or_else(|| first_text_frame(tag, "TRCK")),
        total_tracks: tag
            .total_tracks()
            .map(|number| number.to_string())
            .or_else(|| {
                first_text_frame(tag, "TRCK")
                    .and_then(|trck| trck.split('/').nth(1).map(str::to_string))
            }),
        date: tag
            .year()
            .map(|year| year.to_string())
            .or_else(|| first_text_frame(tag, "TDRC"))
            .or_else(|| first_text_frame(tag, "TYER")),
        custom: read_txxx_map(tag),
        artwork: embedded_artwork(tag),
        fields: id3_fields(tag),
    }
}

fn first_text_frame(tag: &Tag, id: &str) -> Option<String> {
    tag.frames().find_map(|frame| {
        if frame.id() != id {
            return None;
        }

        match frame.content() {
            Content::Text(text) => Some(text.clone()),
            Content::ExtendedText(ext) => Some(ext.value.clone()),
            _ => None,
        }
    })
}

fn read_txxx_map(tag: &Tag) -> BTreeMap<String, String> {
    tag.frames()
        .filter_map(|frame| {
            if frame.id() != "TXXX" {
                return None;
            }

            match frame.content() {
                Content::ExtendedText(ext) => Some((ext.description.clone(), ext.value.clone())),
                _ => None,
            }
        })
        .collect()
}

fn embedded_artwork(tag: &Tag) -> Option<EmbeddedArtwork> {
    tag.pictures().next().map(|picture| EmbeddedArtwork {
        mime_type: picture.mime_type.clone(),
        picture_type: picture.picture_type.to_string(),
        description: picture.description.clone(),
        data: picture.data.clone(),
    })
}

fn id3_fields(tag: &Tag) -> Vec<Id3Field> {
    tag.frames().map(id3_field).collect()
}

fn id3_field(frame: &Frame) -> Id3Field {
    match frame.content() {
        Content::ExtendedText(ext) => Id3Field {
            frame_id: descriptor_frame_label("TXXX", &ext.description),
            value: extended_text_value(&ext.value),
        },
        Content::ExtendedLink(ext) => Id3Field {
            frame_id: descriptor_frame_label("WXXX", &ext.description),
            value: ext.link.clone(),
        },
        Content::Comment(comment) => Id3Field {
            frame_id: descriptor_frame_label("COMM", &comment.description),
            value: comment.text.clone(),
        },
        Content::Lyrics(lyrics) => Id3Field {
            frame_id: descriptor_frame_label("USLT", &lyrics.description),
            value: lyrics.text.clone(),
        },
        Content::SynchronisedLyrics(lyrics) => Id3Field {
            frame_id: descriptor_frame_label("SYLT", &lyrics.description),
            value: synchronised_lyrics_display_value(lyrics),
        },
        Content::UniqueFileIdentifier(ufid) => Id3Field {
            frame_id: descriptor_frame_label("UFID", &ufid.owner_identifier),
            value: String::from_utf8(ufid.identifier.clone())
                .unwrap_or_else(|_| format!("{:x?}", ufid.identifier)),
        },
        Content::InvolvedPeopleList(list) => Id3Field {
            frame_id: frame.id().to_string(),
            value: format_involved_people_list(list),
        },
        _ => Id3Field {
            frame_id: frame.id().to_string(),
            value: frame.content().to_string(),
        },
    }
}

/// A tool such as mutagen ends a `TXXX` value with a NUL terminator, and the
/// `id3` crate keeps it. Without the trim, the JSON of a value routes frame does
/// not parse, and the compare grid shows a difference that is not real.
fn extended_text_value(value: &str) -> String {
    value.trim_end_matches('\0').to_string()
}

/// The text between two pairs of a `TIPL` or `TMCL` edit value.
const INVOLVED_PEOPLE_SEPARATOR: &str = " / ";

/// The text between the role and the name of one pair.
const INVOLVED_ROLE_SEPARATOR: &str = ": ";

/// The cause of a [`BadInvolvedPerson`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvolvedPersonCause {
    /// The role is empty after the trim.
    EmptyRole,
    /// The name is empty after the trim.
    EmptyName,
    /// The role or the name contains `" / "`, the text between two pairs.
    ContainsPairSeparator,
    /// The role contains `": "`, the text between the role and the name.
    RoleContainsRoleSeparator,
    /// The value does not read back as the same pairs for another cause,
    /// for example a name that ends with `" /"`.
    NoSameReadBack,
}

/// A (role, name) pair that does not read back the same from a `TIPL` or
/// `TMCL` edit value. [`involved_people_value`] gives this error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadInvolvedPerson {
    /// The position of the pair in the input, from 0.
    pub index: usize,
    /// The role of the pair, after the trim.
    pub role: String,
    /// The name of the pair, after the trim.
    pub name: String,
    /// Why the pair does not read back the same.
    pub cause: InvolvedPersonCause,
}

impl std::fmt::Display for BadInvolvedPerson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cause = match self.cause {
            InvolvedPersonCause::EmptyRole => "the role is empty",
            InvolvedPersonCause::EmptyName => "the name is empty",
            InvolvedPersonCause::ContainsPairSeparator => "the role or the name contains \" / \"",
            InvolvedPersonCause::RoleContainsRoleSeparator => "the role contains \": \"",
            InvolvedPersonCause::NoSameReadBack => "the value does not read back as the same pairs",
        };
        write!(
            f,
            "involved person {} (role {:?}, name {:?}): {cause}",
            self.index, self.role, self.name
        )
    }
}

impl std::error::Error for BadInvolvedPerson {}

/// Gives the edit value of a `TIPL` or `TMCL` frame from (role, name) pairs,
/// for example `"bass: Josh Moroz / vocals: Josh Moroz / vocals: Alex Nicol"`.
/// The crate owns the text of this value (musicindex ADR 0093 R-TAG-1 item 4),
/// so an app does not join the pairs itself.
///
/// The function trims each role and each name. Two people with the same role
/// give two pairs with that role.
///
/// # Errors
///
/// Gives [`BadInvolvedPerson`] for the first pair that does not read back the
/// same: an empty role or name, a role or name that contains `" / "`, or a
/// role that contains `": "`.
pub fn involved_people_value<'a, I>(pairs: I) -> Result<String, BadInvolvedPerson>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let pairs: Vec<(&str, &str)> = pairs
        .into_iter()
        .map(|(role, name)| (role.trim(), name.trim()))
        .collect();
    let bad = |index: usize, cause| {
        let (role, name) = pairs[index];
        BadInvolvedPerson {
            index,
            role: role.to_string(),
            name: name.to_string(),
            cause,
        }
    };
    for (index, (role, name)) in pairs.iter().enumerate() {
        let cause = if role.is_empty() {
            InvolvedPersonCause::EmptyRole
        } else if name.is_empty() {
            InvolvedPersonCause::EmptyName
        } else if role.contains(INVOLVED_PEOPLE_SEPARATOR)
            || name.contains(INVOLVED_PEOPLE_SEPARATOR)
        {
            InvolvedPersonCause::ContainsPairSeparator
        } else if role.contains(INVOLVED_ROLE_SEPARATOR) {
            InvolvedPersonCause::RoleContainsRoleSeparator
        } else {
            continue;
        };
        return Err(bad(index, cause));
    }
    let value = join_involved_people(pairs.iter().copied());
    let read_back = parse_involved_people_list(&value).items;
    let first_difference = (0..pairs.len()).find(|&index| {
        read_back
            .get(index)
            .is_none_or(|item| (item.involvement.as_str(), item.involvee.as_str()) != pairs[index])
    });
    match first_difference {
        Some(index) => Err(bad(index, InvolvedPersonCause::NoSameReadBack)),
        None if read_back.len() != pairs.len() => Err(bad(
            pairs.len().saturating_sub(1),
            InvolvedPersonCause::NoSameReadBack,
        )),
        None => Ok(value),
    }
}

/// Joins the pairs with the two separators. Only this function and
/// [`parse_involved_people_list`] use the separators.
fn join_involved_people<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    pairs
        .map(|(role, name)| format!("{role}{INVOLVED_ROLE_SEPARATOR}{name}"))
        .collect::<Vec<_>>()
        .join(INVOLVED_PEOPLE_SEPARATOR)
}

/// The display value of a `TIPL` or `TMCL` frame of a file. The data of a
/// file is not checked, so this uses [`join_involved_people`] directly.
fn format_involved_people_list(list: &InvolvedPeopleList) -> String {
    join_involved_people(
        list.items
            .iter()
            .map(|item| (item.involvement.trim(), item.involvee.trim())),
    )
}

fn descriptor_frame_label(frame_id: &str, descriptor: &str) -> String {
    match normalize_frame_descriptor(descriptor) {
        Some(descriptor) => format!("{frame_id}:{descriptor}"),
        None => frame_id.to_string(),
    }
}

fn synchronised_lyrics_display_value(lyrics: &SynchronisedLyrics) -> String {
    if lyrics.content.is_empty() {
        return lyrics.content_type.to_string();
    }
    let mut bytes = Vec::new();
    if lyrics.fmt_table(&mut bytes).is_ok() {
        String::from_utf8(bytes).unwrap_or_else(|_| lyrics.content_type.to_string())
    } else {
        lyrics.content_type.to_string()
    }
}

fn id3v24_edit_frame(edit: &Id3v24Edit, source: &dyn ReferenceSource) -> Result<Frame> {
    let (frame_id, descriptor) = split_frame_label(&edit.frame_label);
    let value = edit.value.trim();
    if frame_id.is_empty() {
        return Err(anyhow!("missing ID3 frame id"));
    }
    if value.is_empty() {
        return Err(anyhow!("missing ID3 value for {frame_id}"));
    }

    let frame_id = frame_id.as_str();
    if !id3v24_edit_frame_is_writable(frame_id) {
        return Err(anyhow!("unsupported ID3v2.4 edit frame {frame_id}"));
    }

    match frame_id {
        "TXXX" => Ok(Frame::with_content(
            "TXXX",
            Content::ExtendedText(ExtendedText {
                description: required_frame_descriptor(frame_id, descriptor)?,
                value: value.to_string(),
            }),
        )),
        "WXXX" => Ok(Frame::with_content(
            "WXXX",
            Content::ExtendedLink(ExtendedLink {
                description: required_frame_descriptor(frame_id, descriptor)?,
                link: value.to_string(),
            }),
        )),
        "UFID" => {
            let owner_identifier = required_frame_descriptor(frame_id, descriptor)?;
            Ok(Frame::with_content(
                "UFID",
                Content::UniqueFileIdentifier(UniqueFileIdentifier {
                    owner_identifier,
                    identifier: value.as_bytes().to_vec(),
                }),
            ))
        }
        "COMM" => Ok(Frame::with_content(
            "COMM",
            Content::Comment(Comment {
                lang: "eng".into(),
                description: required_frame_descriptor(frame_id, descriptor)?,
                text: value.to_string(),
            }),
        )),
        "USLT" => uslt_frame_from_reference(value, descriptor, source),
        "SYLT" => sylt_frame_from_reference(value, descriptor, source),
        "APIC" => apic_frame_from_reference(value, source),
        "TIPL" | "TMCL" => Ok(Frame::with_content(
            frame_id,
            Content::InvolvedPeopleList(parse_involved_people_list(value)),
        )),
        "TIT2" => Ok(Frame::text("TIT2", sanitize_title_text(value))),
        id if id.starts_with('T') => Ok(Frame::text(id, value)),
        id if id.starts_with('W') => Ok(Frame::with_content(
            id,
            id3::frame::Content::Link(value.to_string()),
        )),
        _ => unreachable!("frame writability was checked before construction"),
    }
}

#[must_use]
pub fn id3v24_edit_label_is_writable(frame_label: &str) -> bool {
    let (frame_id, descriptor) = split_frame_label(frame_label);
    if !id3v24_edit_frame_is_writable(&frame_id) {
        return false;
    }
    !matches!(
        frame_id.as_str(),
        "TXXX" | "WXXX" | "UFID" | "COMM" | "USLT" | "SYLT"
    ) || descriptor.is_some()
}

fn id3v24_edit_frame_is_writable(frame_id: &str) -> bool {
    WRITABLE_TEXT_FRAMES.contains(&frame_id)
        || WRITABLE_URL_FRAMES.contains(&frame_id)
        || frame_id == "APIC"
        || matches!(frame_id, "COMM" | "USLT" | "SYLT")
        || frame_id == "UFID"
        || matches!(frame_id, "TIPL" | "TMCL")
}

fn required_frame_descriptor(frame_id: &str, descriptor: Option<String>) -> Result<String> {
    descriptor.ok_or_else(|| anyhow!("{frame_id} edits require a descriptor"))
}

/// Parse a `"role: name / role: name"` display string back into an [`InvolvedPeopleList`].
fn parse_involved_people_list(value: &str) -> InvolvedPeopleList {
    let items = value
        .split(INVOLVED_PEOPLE_SEPARATOR)
        .filter_map(|entry| {
            let (involvement, involvee) = entry.split_once(INVOLVED_ROLE_SEPARATOR)?;
            Some(InvolvedPeopleListItem {
                involvement: involvement.trim().to_string(),
                involvee: involvee.trim().to_string(),
            })
        })
        .collect();
    InvolvedPeopleList { items }
}

#[must_use]
pub fn sanitize_title_text(value: &str) -> String {
    value
        .trim_start()
        .strip_prefix("- ")
        .map_or(value, str::trim_start)
        .to_string()
}

fn split_frame_label(label: &str) -> (String, Option<String>) {
    let Some((frame_id, descriptor)) = label.split_once(':') else {
        return (label.trim().to_ascii_uppercase(), None);
    };
    (
        frame_id.trim().to_ascii_uppercase(),
        normalize_frame_descriptor(descriptor),
    )
}

fn normalize_frame_descriptor(descriptor: &str) -> Option<String> {
    let descriptor = descriptor.replace('\0', " ");
    let descriptor = descriptor.split_whitespace().collect::<Vec<_>>().join(" ");
    (!descriptor.is_empty()).then(|| descriptor.clone())
}

fn apic_frame_from_reference(reference: &str, source: &dyn ReferenceSource) -> Result<Frame> {
    let (mime_type, data) = read_picture_reference(reference, source)?;
    Ok(Frame::with_content(
        "APIC",
        Content::Picture(Picture {
            mime_type,
            picture_type: PictureType::CoverFront,
            description: "front cover".into(),
            data,
        }),
    ))
}

fn uslt_frame_from_reference(
    reference: &str,
    descriptor: Option<String>,
    source: &dyn ReferenceSource,
) -> Result<Frame> {
    let description = required_frame_descriptor("USLT", descriptor)?;
    let transcript = parse_transcript_reference(reference, source)?;
    Ok(Frame::with_content(
        "USLT",
        Content::Lyrics(Lyrics {
            lang: "eng".into(),
            description,
            text: transcript.plain_text,
        }),
    ))
}

fn sylt_frame_from_reference(
    reference: &str,
    descriptor: Option<String>,
    source: &dyn ReferenceSource,
) -> Result<Frame> {
    let description = required_frame_descriptor("SYLT", descriptor)?;
    let transcript = parse_transcript_reference(reference, source)?;
    let content = if transcript.timed_lines.is_empty() {
        vec![(0, transcript.plain_text)]
    } else {
        transcript.timed_lines
    };
    Ok(Frame::with_content(
        "SYLT",
        Content::SynchronisedLyrics(SynchronisedLyrics {
            lang: "eng".into(),
            timestamp_format: TimestampFormat::Ms,
            content_type: SynchronisedLyricsType::Transcription,
            description,
            content,
        }),
    ))
}

#[derive(Debug, Default)]
struct ParsedTranscript {
    timed_lines: Vec<(u32, String)>,
    plain_text: String,
}

fn parse_transcript_reference(
    reference: &str,
    source: &dyn ReferenceSource,
) -> Result<ParsedTranscript> {
    let text = source
        .text(reference)
        .with_context(|| format!("read transcript {reference}"))?;
    let mut parsed = parse_srt_or_vtt_transcript(&text);
    if parsed.timed_lines.is_empty() {
        parsed = parse_lrc_transcript(&text);
    }
    if parsed.timed_lines.is_empty() {
        parsed = parse_microdvd_sub_transcript(&text);
    }
    if parsed.plain_text.trim().is_empty() {
        parsed.plain_text = strip_timecode_data(&text);
    }
    if parsed.plain_text.trim().is_empty() {
        return Err(anyhow!("transcript is empty"));
    }
    Ok(parsed)
}

fn parse_srt_or_vtt_transcript(text: &str) -> ParsedTranscript {
    let mut timed_lines = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let lines = block
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && *line != "WEBVTT")
            .collect::<Vec<_>>();
        let Some(timestamp_index) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let Some(start) = lines[timestamp_index]
            .split("-->")
            .next()
            .and_then(parse_timecode_ms)
        else {
            continue;
        };
        let text = lines
            .iter()
            .skip(timestamp_index + 1)
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        if !text.is_empty() {
            timed_lines.push((start, text));
        }
    }
    transcript_from_timed_lines(timed_lines)
}

fn parse_lrc_transcript(text: &str) -> ParsedTranscript {
    let mut timed_lines = Vec::new();
    for line in text.lines() {
        let mut rest = line.trim();
        let mut starts = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some((stamp, remaining)) = stripped.split_once(']') else {
                break;
            };
            let Some(ms) = parse_lrc_timecode_ms(stamp) else {
                break;
            };
            starts.push(ms);
            rest = remaining.trim_start();
        }
        if rest.is_empty() {
            continue;
        }
        timed_lines.extend(starts.into_iter().map(|start| (start, rest.to_string())));
    }
    transcript_from_timed_lines(timed_lines)
}

fn parse_microdvd_sub_transcript(text: &str) -> ParsedTranscript {
    let mut timed_lines = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some(after_open) = line.strip_prefix('{') else {
            continue;
        };
        let Some((start_frame, rest)) = after_open.split_once('}') else {
            continue;
        };
        let Some(rest) = rest.strip_prefix('{') else {
            continue;
        };
        let Some((_end_frame, text)) = rest.split_once('}') else {
            continue;
        };
        let Ok(start_frame) = start_frame.parse::<u32>() else {
            continue;
        };
        let text = text.replace('|', " ").trim().to_string();
        if !text.is_empty() {
            timed_lines.push((start_frame.saturating_mul(40), text));
        }
    }
    transcript_from_timed_lines(timed_lines)
}

fn transcript_from_timed_lines(timed_lines: Vec<(u32, String)>) -> ParsedTranscript {
    let plain_text = timed_lines
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    ParsedTranscript {
        timed_lines,
        plain_text,
    }
}

fn parse_timecode_ms(value: &str) -> Option<u32> {
    let time = value
        .split_whitespace()
        .next()
        .unwrap_or("")
        .replace(',', ".");
    let mut parts = time.split(':').collect::<Vec<_>>();
    if parts.len() == 2 {
        parts.insert(0, "0");
    }
    let [hours, minutes, seconds] = parts.as_slice() else {
        return None;
    };
    let hours = hours.parse::<u32>().ok()?;
    let minutes = minutes.parse::<u32>().ok()?;
    let (seconds, millis) = seconds
        .split_once('.')
        .map_or((*seconds, "0"), |(seconds, millis)| (seconds, millis));
    let seconds = seconds.parse::<u32>().ok()?;
    let millis = parse_millis(millis)?;
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(seconds.checked_mul(1_000)?)?
        .checked_add(millis)
}

fn parse_lrc_timecode_ms(value: &str) -> Option<u32> {
    if !value.chars().next()?.is_ascii_digit() {
        return None;
    }
    parse_timecode_ms(&format!("0:{value}"))
}

fn parse_millis(value: &str) -> Option<u32> {
    let digits = value
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        return Some(0);
    }
    let mut padded = digits;
    while padded.len() < 3 {
        padded.push('0');
    }
    padded.get(..3)?.parse::<u32>().ok()
}

fn strip_timecode_data(text: &str) -> String {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed == "WEBVTT"
                || trimmed.chars().all(|ch| ch.is_ascii_digit())
                || trimmed.contains("-->")
            {
                return None;
            }
            if trimmed.starts_with('[') && trimmed.contains(']') {
                return trimmed
                    .rsplit(']')
                    .next()
                    .map(str::trim)
                    .filter(|text| !text.is_empty());
            }
            if trimmed.starts_with('{') && trimmed.contains('}') {
                return trimmed
                    .rsplit('}')
                    .next()
                    .map(str::trim)
                    .filter(|text| !text.is_empty());
            }
            Some(trimmed)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The MIME type and the bytes of the picture at `reference`. An empty
/// picture is an error.
fn read_picture_reference(
    reference: &str,
    source: &dyn ReferenceSource,
) -> Result<(String, Vec<u8>)> {
    let (mime_type, data) = source
        .picture(reference)
        .with_context(|| format!("read APIC image {reference}"))?;
    if data.is_empty() {
        return Err(anyhow!("APIC image is empty"));
    }
    Ok((mime_type, data))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use id3::frame::{ExtendedText, Picture, PictureType};
    use id3::{Frame, Tag, TagLike};
    use lofty::prelude::ItemKey;

    use super::{
        AudioTags, EmbeddedArtwork, Id3Field, Id3v24Edit, TagWriteResult,
        add_lofty_compare_aliases, audio_tags_from_id3, id3v24_edit_label_is_writable,
        lofty_field_for_label, lofty_item_label, lofty_mp4_freeform_key,
        normalize_frame_descriptor, read_audio_tags, resolve_license_edits, vorbis_storage_key,
    };
    use crate::source::ReferenceSource;

    /// A source that reads local files. HitIt ADR 0056 gives the network
    /// source of the app, and the app tests it.
    struct LocalFiles;

    impl ReferenceSource for LocalFiles {
        fn text(&self, reference: &str) -> anyhow::Result<String> {
            Ok(fs::read_to_string(reference)?)
        }

        fn picture(&self, reference: &str) -> anyhow::Result<(String, Vec<u8>)> {
            Ok(("image/png".into(), fs::read(reference)?))
        }
    }

    fn write_id3v24_edits(
        path: &std::path::Path,
        edits: &[Id3v24Edit],
    ) -> anyhow::Result<TagWriteResult> {
        super::write_id3v24_edits(path, edits, &LocalFiles)
    }

    #[test]
    fn extended_text_value_drops_the_nul_terminator_of_mutagen() {
        let frame = Frame::with_content(
            "TXXX",
            id3::Content::ExtendedText(ExtendedText {
                description: "Podcast Value Routes".into(),
                value: "[{\"split\": 40}]\0".into(),
            }),
        );
        let field = super::id3_field(&frame);
        assert_eq!(field.value, "[{\"split\": 40}]");
        assert!(serde_json::from_str::<serde_json::Value>(&field.value).is_ok());
    }

    #[test]
    fn maps_id3_frames_to_audio_tags() {
        let mut tag = Tag::new();
        tag.set_title("Song Title");
        tag.set_artist("Track Artist");
        tag.set_album("Feed Title");
        tag.set_track(7);
        tag.set_total_tracks(10);
        tag.set_year(2026);
        tag.add_frame(Frame::with_content(
            "TXXX",
            id3::Content::ExtendedText(ExtendedText {
                description: "V4V_PUBLISHER".into(),
                value: "Wavlake".into(),
            }),
        ));
        tag.add_frame(Frame::with_content(
            "APIC",
            id3::Content::Picture(Picture {
                mime_type: "image/png".into(),
                picture_type: PictureType::CoverFront,
                description: "front".into(),
                data: vec![1, 2, 3],
            }),
        ));

        assert_eq!(
            audio_tags_from_id3(&tag),
            AudioTags {
                title: Some("Song Title".into()),
                artist: Some("Track Artist".into()),
                album: Some("Feed Title".into()),
                track_number: Some("7".into()),
                total_tracks: Some("10".into()),
                date: Some("2026".into()),
                custom: BTreeMap::from([("V4V_PUBLISHER".into(), "Wavlake".into())]),
                artwork: Some(EmbeddedArtwork {
                    mime_type: "image/png".into(),
                    picture_type: "Front cover".into(),
                    description: "front".into(),
                    data: vec![1, 2, 3],
                }),
                fields: vec![
                    Id3Field {
                        frame_id: "TIT2".into(),
                        value: "Song Title".into(),
                    },
                    Id3Field {
                        frame_id: "TPE1".into(),
                        value: "Track Artist".into(),
                    },
                    Id3Field {
                        frame_id: "TALB".into(),
                        value: "Feed Title".into(),
                    },
                    Id3Field {
                        frame_id: "TRCK".into(),
                        value: "7/10".into(),
                    },
                    Id3Field {
                        frame_id: "TYER".into(),
                        value: "2026".into(),
                    },
                    Id3Field {
                        frame_id: "TXXX:V4V_PUBLISHER".into(),
                        value: "Wavlake".into(),
                    },
                    Id3Field {
                        frame_id: "APIC".into(),
                        value: "front: Front cover (image/png, 3 bytes)".into(),
                    },
                ],
            }
        );
    }

    #[test]
    fn missing_id3_tag_returns_blank_tags() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");

        assert_eq!(
            read_audio_tags(temp.path()).expect("read blank tags"),
            AudioTags::default()
        );
    }

    #[test]
    fn lofty_unknown_artist_webpage_maps_back_to_woar() {
        assert_eq!(
            lofty_item_label(&ItemKey::Unknown("ARTISTWEBPAGE".into())),
            "WOAR"
        );
        assert_eq!(
            lofty_item_label(&ItemKey::Unknown("----:com.apple.iTunes:WOAR".into())),
            "WOAR"
        );
    }

    /// R80-57 (HitIt ADR 0080 Decision 9), the MP4 half: the image frame gets a
    /// stable MP4 freeform atom key, under the same scheme as every other
    /// `TXXX:MusicIndex` frame (Contributors, Value Routes, the two GUID
    /// frames), and the reader maps that key back to the same frame label.
    #[test]
    fn lofty_podcast_image_round_trips_its_mp4_freeform_key() {
        let field = lofty_field_for_label("TXXX:Podcast Image");
        let key = lofty_mp4_freeform_key("TXXX:Podcast Image", &field);
        assert_eq!(key, "----:com.apple.iTunes:Podcast Image");
        assert_eq!(
            lofty_item_label(&ItemKey::Unknown(key)),
            "TXXX:Podcast Image"
        );
    }

    /// R80-57 (HitIt ADR 0080 Decision 9), the Vorbis half: the image frame gets
    /// a stable Vorbis Comment key.
    #[test]
    fn vorbis_storage_key_maps_the_podcast_image_frame() {
        assert_eq!(
            vorbis_storage_key("TXXX:Podcast Image", crate::format::AudioFormat::Flac),
            Some("PODCAST IMAGE".into())
        );
    }

    #[test]
    fn lofty_aliases_fill_descriptor_and_artwork_presence_gaps() {
        let mut fields = vec![
            Id3Field {
                frame_id: "COMM".into(),
                value: "MusicIndex description".into(),
            },
            Id3Field {
                frame_id: "USLT".into(),
                value: "Embedded transcript".into(),
            },
        ];
        let artwork = EmbeddedArtwork {
            mime_type: "image/jpeg".into(),
            picture_type: "CoverFront".into(),
            description: "front cover".into(),
            data: vec![1, 2, 3],
        };

        add_lofty_compare_aliases(&mut fields, Some(&artwork));

        assert!(
            fields
                .iter()
                .any(|field| field.frame_id == "COMM:Podcast Description")
        );
        assert!(
            fields
                .iter()
                .any(|field| field.frame_id == "USLT:Podcast Transcript")
        );
        assert!(
            fields
                .iter()
                .any(|field| field.frame_id == "SYLT:Podcast Transcript")
        );
        assert!(fields.iter().any(|field| field.frame_id == "APIC"));
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "one test of each frame kind")]
    fn writes_staged_id3v24_text_extended_url_and_ufid_frames() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        let image = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .expect("temp image file");
        fs::write(image.path(), [1, 2, 3]).expect("write image file");
        let transcript = tempfile::Builder::new()
            .suffix(".srt")
            .tempfile()
            .expect("temp transcript file");
        fs::write(
            transcript.path(),
            "1\n00:00:01,250 --> 00:00:03,000\nHello world\n",
        )
        .expect("write transcript file");

        let edits = [
            Id3v24Edit {
                frame_label: "TIT2".into(),
                value: " - Song".into(),
            },
            Id3v24Edit {
                frame_label: "TXXX:Podcast Contributors".into(),
                value: "Alice".into(),
            },
            Id3v24Edit {
                frame_label: "WOAR".into(),
                value: "https://example.test".into(),
            },
            Id3v24Edit {
                frame_label: "WOAR".into(),
                value: "https://musicbrainz.example.test".into(),
            },
            Id3v24Edit {
                frame_label: "UFID:http://musicbrainz.org".into(),
                value: "recording-id".into(),
            },
            Id3v24Edit {
                frame_label: "APIC".into(),
                value: image.path().display().to_string(),
            },
            Id3v24Edit {
                frame_label: "COMM:Podcast Description".into(),
                value: "RSS description".into(),
            },
            Id3v24Edit {
                frame_label: "USLT:Podcast Transcript".into(),
                value: transcript.path().display().to_string(),
            },
            Id3v24Edit {
                frame_label: "SYLT:Podcast Transcript".into(),
                value: transcript.path().display().to_string(),
            },
            Id3v24Edit {
                frame_label: "TMCL".into(),
                value: "guitar: Alice / vocals: Bob".into(),
            },
        ];

        assert_eq!(
            write_id3v24_edits(temp.path(), &edits)
                .expect("write ID3v2.4 edits")
                .applied,
            10
        );

        let tag = Tag::read_from_path(temp.path()).expect("read written ID3 tag");
        assert_eq!(tag.title(), Some("Song"));
        assert!(tag.frames().any(|frame| {
            frame.id() == "TXXX" && frame.content().to_string() == "Podcast Contributors: Alice"
        }));
        assert!(
            tag.frames().any(|frame| frame.id() == "WOAR"
                && frame.content().to_string() == "https://example.test")
        );
        let woar_values = tag
            .frames()
            .filter(|frame| frame.id() == "WOAR")
            .map(|frame| frame.content().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            woar_values,
            vec![
                "https://example.test".to_string(),
                "https://musicbrainz.example.test".to_string()
            ]
        );
        assert!(tag.frames().any(|frame| {
            frame.id() == "UFID"
                && frame.content().to_string() == "http://musicbrainz.org: recording-id"
        }));
        assert!(tag.frames().any(|frame| {
            frame.id() == "APIC"
                && frame.content().to_string() == "front cover: Front cover (image/png, 3 bytes)"
        }));
        assert!(tag.frames().any(|frame| {
            frame.id() == "COMM"
                && frame.content().to_string() == "Podcast Description: RSS description"
        }));
        assert!(tag.frames().any(|frame| {
            frame.id() == "USLT" && frame.content().to_string() == "Podcast Transcript: Hello world"
        }));
        assert!(tag.frames().any(|frame| {
            matches!(
                frame.content(),
                id3::Content::SynchronisedLyrics(lyrics)
                    if frame.id() == "SYLT"
                        && lyrics.content == vec![(1250, "Hello world".to_string())]
            )
        }));
        assert!(tag.frames().any(|frame| {
            matches!(
                frame.content(),
                id3::Content::InvolvedPeopleList(list)
                    if frame.id() == "TMCL"
                        && list.items.len() == 2
                        && list.items[0].involvement == "guitar"
                        && list.items[0].involvee == "Alice"
                        && list.items[1].involvement == "vocals"
                        && list.items[1].involvee == "Bob"
            )
        }));
    }

    #[test]
    fn rejects_unwritable_or_underspecified_id3v24_edit_labels() {
        assert!(id3v24_edit_label_is_writable("TIT2"));
        assert!(id3v24_edit_label_is_writable("TXXX:Podcast Contributors"));
        assert!(id3v24_edit_label_is_writable("WXXX:Official audio"));
        assert!(id3v24_edit_label_is_writable("UFID:http://musicbrainz.org"));
        assert!(id3v24_edit_label_is_writable("APIC"));
        assert!(id3v24_edit_label_is_writable("COMM:Podcast Description"));
        assert!(id3v24_edit_label_is_writable("SYLT:Podcast Transcript"));
        assert!(id3v24_edit_label_is_writable("USLT:Podcast Transcript"));
        assert!(id3v24_edit_label_is_writable("TYER"));
        assert!(!id3v24_edit_label_is_writable("TXXX"));
        assert!(!id3v24_edit_label_is_writable("WXXX"));
        assert!(!id3v24_edit_label_is_writable("UFID"));
        assert!(!id3v24_edit_label_is_writable("COMM"));
        assert!(!id3v24_edit_label_is_writable("SYLT"));
        assert!(!id3v24_edit_label_is_writable("USLT"));
        assert!(!id3v24_edit_label_is_writable("TFOO"));

        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        let edits = [Id3v24Edit {
            frame_label: "TFOO".into(),
            value: "not writable".into(),
        }];

        let error = write_id3v24_edits(temp.path(), &edits)
            .expect_err("unsupported ID3v2.4 edits should be rejected");
        assert!(
            error.to_string().contains("unsupported ID3v2.4"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn frame_descriptors_strip_nulls_and_whitespace() {
        assert_eq!(
            normalize_frame_descriptor(" \0Podcast Contributors\0 "),
            Some("Podcast Contributors".into())
        );
    }

    fn woar_frame_values(tag: &Tag) -> Vec<String> {
        let mut values = tag
            .frames()
            .filter(|frame| frame.id() == "WOAR")
            .map(|frame| frame.content().link().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        values.sort();
        values
    }

    fn woaf_frame_values(tag: &Tag) -> Vec<String> {
        tag.frames()
            .filter(|frame| frame.id() == "WOAF")
            .map(|frame| frame.content().link().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
    }

    /// R80-06: a write of the same edits twice to an MP3 file gives equal
    /// frames. The second write adds no duplicate.
    #[test]
    fn adr_0080_repeated_mp3_write_gives_equal_frames() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        let edits = [
            Id3v24Edit {
                frame_label: "TIT2".into(),
                value: "Song".into(),
            },
            Id3v24Edit {
                frame_label: "TALB".into(),
                value: "Feed Title".into(),
            },
            Id3v24Edit {
                frame_label: "TXXX:RSS Nostr Handle".into(),
                value: "npub1example".into(),
            },
            Id3v24Edit {
                frame_label: "WOAR".into(),
                value: "https://example.test/feed".into(),
            },
            Id3v24Edit {
                frame_label: "WOAF".into(),
                value: "https://example.test/track".into(),
            },
            Id3v24Edit {
                frame_label: "COMM:Podcast Description".into(),
                value: "Track description".into(),
            },
        ];

        write_id3v24_edits(temp.path(), &edits).expect("first write");
        let first = read_audio_tags(temp.path())
            .expect("read after first write")
            .fields;
        write_id3v24_edits(temp.path(), &edits).expect("second write");
        let second = read_audio_tags(temp.path())
            .expect("read after second write")
            .fields;

        assert_eq!(first, second, "a repeated write must not duplicate a frame");
    }

    /// R80-06: the same idempotent-write proof for FLAC, on a copy of a real
    /// FLAC fixture in a temporary file.
    #[test]
    fn adr_0080_repeated_flac_write_gives_equal_fields() {
        let flac_bytes = include_bytes!("../tests/fixtures/conversion.flac");
        let temp = tempfile::Builder::new()
            .suffix(".flac")
            .tempfile()
            .expect("temp flac file");
        fs::write(temp.path(), flac_bytes).expect("write flac fixture copy");
        let edits = [
            Id3v24Edit {
                frame_label: "TIT2".into(),
                value: "Song".into(),
            },
            Id3v24Edit {
                frame_label: "TALB".into(),
                value: "Feed Title".into(),
            },
            Id3v24Edit {
                frame_label: "TXXX:RSS Nostr Handle".into(),
                value: "npub1example".into(),
            },
            Id3v24Edit {
                frame_label: "WOAR".into(),
                value: "https://example.test/feed".into(),
            },
            Id3v24Edit {
                frame_label: "WOAF".into(),
                value: "https://example.test/track".into(),
            },
        ];

        write_id3v24_edits(temp.path(), &edits).expect("first write");
        let first = read_audio_tags(temp.path())
            .expect("read after first write")
            .fields;
        write_id3v24_edits(temp.path(), &edits).expect("second write");
        let second = read_audio_tags(temp.path())
            .expect("read after second write")
            .fields;

        assert_eq!(
            first, second,
            "a repeated FLAC write must not duplicate a field"
        );
    }

    /// A tag file whose `TLAN` frame header starts two bytes before the end of
    /// the first 8 KiB read buffer. The bytes hold `TIT2`, then `USLT`, then
    /// `TLAN`, then `TBPM`.
    fn tag_with_frame_header_across_read_buffer() -> Vec<u8> {
        const BUFFER_SIZE: usize = 8 * 1024;
        let encode = |text_len: usize| {
            let mut tag = Tag::new();
            tag.set_title("Song");
            tag.add_frame(id3::frame::Lyrics {
                lang: "eng".into(),
                description: String::new(),
                text: "x".repeat(text_len),
            });
            tag.add_frame(Frame::text("TLAN", "eng"));
            tag.add_frame(Frame::text("TBPM", "95"));
            let mut bytes = Vec::new();
            tag.write_to(&mut bytes, id3::Version::Id3v24)
                .expect("encode tag");
            bytes
        };
        let position = |bytes: &[u8]| {
            bytes
                .windows(4)
                .position(|window| window == b"TLAN")
                .expect("TLAN frame")
        };
        let text_len = BUFFER_SIZE - 2 - position(&encode(0));
        let bytes = encode(text_len);
        assert_eq!(position(&bytes), BUFFER_SIZE - 2);
        bytes
    }

    /// HitIt ADR 0080 Decision 6: the operator found two files on 2026-11-05 that
    /// showed each frame after one position as "no value". `id3` 1.16.4 took
    /// a short read of a frame header as the end of the tag. A write on such
    /// a read removes each later frame.
    #[test]
    fn adr_0080_frame_header_across_read_buffer_keeps_each_later_frame() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("song.mp3");
        std::fs::write(&path, tag_with_frame_header_across_read_buffer()).expect("write file");

        let tags = read_audio_tags(&path).expect("read tags");
        for label in ["TLAN", "TBPM"] {
            assert!(
                tags.fields.iter().any(|field| field.frame_id == label),
                "the reader must give {label}"
            );
        }

        super::write_mp3_edits(
            &path,
            &[Id3v24Edit {
                frame_label: "TPUB".into(),
                value: "Label".into(),
            }],
            None,
            &LocalFiles,
        )
        .expect("write edit");
        let tag = Tag::read_from_path(&path).expect("read written tag");
        for id in ["TIT2", "USLT", "TLAN", "TBPM", "TPUB"] {
            assert!(tag.get(id).is_some(), "the write must keep {id}");
        }
    }

    /// Orchestrator review, defect 2: two edits that alias to the same
    /// Vorbis key (`TDRC` and `TYER`, both `DATE`) keep both their values.
    /// A once-per-edit removal would let the second edit erase the first.
    #[test]
    fn adr_0080_two_edits_sharing_one_vorbis_key_keep_both_values() {
        let flac_bytes = include_bytes!("../tests/fixtures/conversion.flac");
        let temp = tempfile::Builder::new()
            .suffix(".flac")
            .tempfile()
            .expect("temp flac file");
        fs::write(temp.path(), flac_bytes).expect("write flac fixture copy");
        let edits = [
            Id3v24Edit {
                frame_label: "TDRC".into(),
                value: "2024-01-01".into(),
            },
            Id3v24Edit {
                frame_label: "TYER".into(),
                value: "2024".into(),
            },
        ];

        write_id3v24_edits(temp.path(), &edits).expect("write edits");
        let tags = read_audio_tags(temp.path()).expect("read tags back");

        let dates = tags
            .fields
            .iter()
            .filter(|field| field.frame_id == "TDRC")
            .map(|field| field.value.as_str())
            .collect::<Vec<_>>();
        assert!(
            dates.contains(&"2024-01-01"),
            "the TDRC value must stay: {dates:?}"
        );
        assert!(
            dates.contains(&"2024"),
            "the TYER value must also stay: {dates:?}"
        );
        assert_eq!(
            dates.len(),
            2,
            "both values must be present, not one replacing the other: {dates:?}"
        );
    }

    /// R80-07: an MP3 file with the earlier labeled channel value and the
    /// item page mistakenly held in `WOAR` holds only the plain channel
    /// value in `WOAR`, and the item page in `WOAF`, after one write.
    #[test]
    fn adr_0080_write_moves_earlier_woar_values_to_their_frame() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        let channel_url = "https://example.test/feed";
        let item_page = "https://example.test/track";
        write_id3v24_edits(
            temp.path(),
            &[
                Id3v24Edit {
                    frame_label: "WOAR".into(),
                    value: format!("download for free (url, forward): {channel_url}"),
                },
                Id3v24Edit {
                    frame_label: "WOAR".into(),
                    value: item_page.into(),
                },
            ],
        )
        .expect("seed the earlier mapping");

        write_id3v24_edits(
            temp.path(),
            &[
                Id3v24Edit {
                    frame_label: "WOAR".into(),
                    value: channel_url.into(),
                },
                Id3v24Edit {
                    frame_label: "WOAF".into(),
                    value: item_page.into(),
                },
            ],
        )
        .expect("write the current mapping");

        let tag = Tag::read_from_path(temp.path()).expect("read written tag");
        assert_eq!(
            woar_frame_values(&tag),
            vec![channel_url.to_string()],
            "WOAR must hold only the plain channel value"
        );
        assert_eq!(
            woaf_frame_values(&tag),
            vec![item_page.to_string()],
            "WOAF must hold only the item page"
        );
    }

    /// HitIt ADR 0080 Decision 6: the value of this write replaces a `WOAR` value
    /// from another tool and from `MusicBrainz`, and the earlier labeled channel
    /// value. The item page goes to its own frame.
    #[test]
    fn adr_0080_write_replaces_each_other_woar_value() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        let foreign_url = "https://foreign.example/from-another-tool";
        let musicbrainz_value =
            "download for free (url, forward): https://musicbrainz.example/artist";
        let channel_url = "https://example.test/feed";
        let item_page = "https://example.test/track";
        write_id3v24_edits(
            temp.path(),
            &[
                Id3v24Edit {
                    frame_label: "WOAR".into(),
                    value: foreign_url.into(),
                },
                Id3v24Edit {
                    frame_label: "WOAR".into(),
                    value: musicbrainz_value.into(),
                },
                Id3v24Edit {
                    frame_label: "WOAR".into(),
                    value: format!("download for free (url, forward): {channel_url}"),
                },
                Id3v24Edit {
                    frame_label: "WOAR".into(),
                    value: item_page.into(),
                },
            ],
        )
        .expect("seed the file with three owners and the earlier mapping");

        write_id3v24_edits(
            temp.path(),
            &[
                Id3v24Edit {
                    frame_label: "WOAR".into(),
                    value: channel_url.into(),
                },
                Id3v24Edit {
                    frame_label: "WOAF".into(),
                    value: item_page.into(),
                },
            ],
        )
        .expect("write the current mapping");

        let tag = Tag::read_from_path(temp.path()).expect("read written tag");
        assert_eq!(
            woar_frame_values(&tag),
            vec![channel_url.to_string()],
            "the value of this write replaces each other WOAR value"
        );
        assert_eq!(woaf_frame_values(&tag), vec![item_page.to_string()]);
    }

    /// R82-08: a write with the RSS channel website and a `MusicBrainz`
    /// release-group homepage holds both plain URLs in `WOAR`. A second
    /// write gives equal frames.
    #[test]
    fn adr_0080_mb_write_keeps_channel_and_musicbrainz_homepage_in_woar() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        let channel_url = "https://example.test/feed";
        let homepage_url = "https://musicbrainz.example/homepage";
        let edits = [
            Id3v24Edit {
                frame_label: "WOAR".into(),
                value: channel_url.into(),
            },
            Id3v24Edit {
                frame_label: "WOAR".into(),
                value: homepage_url.into(),
            },
        ];

        write_id3v24_edits(temp.path(), &edits).expect("first write");
        let tag = Tag::read_from_path(temp.path()).expect("read written tag");
        let mut expected = vec![channel_url.to_string(), homepage_url.to_string()];
        expected.sort();
        assert_eq!(
            woar_frame_values(&tag),
            expected,
            "WOAR must hold both the channel website and the MusicBrainz homepage"
        );

        let first = read_audio_tags(temp.path())
            .expect("read after first write")
            .fields;
        write_id3v24_edits(temp.path(), &edits).expect("second write");
        let second = read_audio_tags(temp.path())
            .expect("read after second write")
            .fields;
        assert_eq!(
            first, second,
            "a repeated write must not duplicate a WOAR frame"
        );
    }

    /// The audio bytes after each fixture tag: the header of one MPEG frame
    /// and a few data bytes. The first byte is not zero, so no reader takes
    /// it as tag padding.
    const OLD_ITUNES_AUDIO: &[u8] = &[
        0xFF, 0xFB, 0x90, 0x64, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA,
    ];

    fn syncsafe(size: usize) -> [u8; 4] {
        let size = u32::try_from(size).expect("fixture size");
        [
            u8::try_from((size >> 21) & 0x7F).expect("byte"),
            u8::try_from((size >> 14) & 0x7F).expect("byte"),
            u8::try_from((size >> 7) & 0x7F).expect("byte"),
            u8::try_from(size & 0x7F).expect("byte"),
        ]
    }

    /// An ID3v2.2 text frame with ISO-8859-1 text.
    fn v22_text_frame(id: [u8; 3], text: &str) -> Vec<u8> {
        let size = u32::try_from(text.len() + 1).expect("frame size");
        let mut frame = id.to_vec();
        frame.extend_from_slice(&size.to_be_bytes()[1..]);
        frame.push(0);
        frame.extend_from_slice(text.as_bytes());
        frame
    }

    /// An ID3v2.4 frame with no flags.
    fn v24_frame(id: [u8; 4], content: &[u8]) -> Vec<u8> {
        let mut frame = id.to_vec();
        frame.extend_from_slice(&syncsafe(content.len()));
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(content);
        frame
    }

    /// A file of one `ID3v2` tag with `frames`, followed by the fixture audio.
    fn tagged_fixture(major_version: u8, frames: &[Vec<u8>]) -> Vec<u8> {
        let body = frames.concat();
        let mut bytes = b"ID3".to_vec();
        bytes.extend_from_slice(&[major_version, 0, 0]);
        bytes.extend_from_slice(&syncsafe(body.len()));
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(OLD_ITUNES_AUDIO);
        bytes
    }

    /// The bytes that follow the `ID3v2` tag at the start of `bytes`.
    fn audio_after_tag(bytes: &[u8]) -> &[u8] {
        assert_eq!(&bytes[..3], b"ID3", "the file must start with a tag");
        let size = bytes[6..10]
            .iter()
            .fold(0_usize, |size, byte| (size << 7) | usize::from(*byte));
        &bytes[10 + size..]
    }

    fn old_itunes_edits() -> Vec<Id3v24Edit> {
        [
            ("TIT2", "Make It"),
            ("TALB", "Disco Swag - The Album"),
            ("TPE1", "The Doerfels"),
            ("TXXX:RSS Item GUID", "item-guid"),
        ]
        .into_iter()
        .map(|(frame_label, value)| Id3v24Edit {
            frame_label: frame_label.into(),
            value: value.into(),
        })
        .collect()
    }

    /// R80-4-01: a write on a file with an ID3v2.2 tag from iTunes gives an
    /// ID3v2.4 tag. The `TSP` value moves to `TSOP`, the audio bytes stay
    /// equal, and the file mode stays the same.
    #[test]
    fn adr_0080_old_itunes_frames_v22_sort_frame_becomes_tsop() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("make-it.mp3");
        let fixture = tagged_fixture(
            2,
            &[
                v22_text_frame(*b"TT2", "Make It (old)"),
                v22_text_frame(*b"TSP", "Doerfels"),
            ],
        );
        fs::write(&path, &fixture).expect("write fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("set mode");

        write_id3v24_edits(&path, &old_itunes_edits()).expect("write tags");

        let tag = Tag::read_from_path(&path).expect("read written tag");
        assert_eq!(tag.version(), id3::Version::Id3v24);
        assert_eq!(tag.title(), Some("Make It"));
        assert_eq!(tag.album(), Some("Disco Swag - The Album"));
        assert_eq!(tag.artist(), Some("The Doerfels"));
        assert_eq!(
            tag.get("TSOP").and_then(|frame| frame.content().text()),
            Some("Doerfels")
        );
        assert!(
            tag.extended_texts()
                .any(|text| text.description == "RSS Item GUID" && text.value == "item-guid")
        );
        let written = fs::read(&path).expect("read written file");
        assert_eq!(audio_after_tag(&written), OLD_ITUNES_AUDIO);
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }

    /// R80-4-03: a write that fails during the encode leaves each byte of
    /// the file unchanged and leaves no staged copy. The fixture `MLLT`
    /// frame decodes, but its field widths cannot be encoded.
    #[test]
    fn adr_0080_old_itunes_frames_failed_encode_leaves_file_unchanged() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("track.mp3");
        let mllt = [0, 1, 0, 0, 1, 0, 0, 1, 1, 1];
        let fixture = tagged_fixture(
            4,
            &[
                v24_frame(*b"TIT2", b"\x03Old title"),
                v24_frame(*b"MLLT", &mllt),
            ],
        );
        fs::write(&path, &fixture).expect("write fixture");

        let result = write_id3v24_edits(&path, &old_itunes_edits());

        assert!(result.is_err(), "the encode must fail: {result:?}");
        assert_eq!(fs::read(&path).expect("read file"), fixture);
        let entries = fs::read_dir(temp.path()).expect("read dir").count();
        assert_eq!(entries, 1, "a failed write must leave no staged copy");
    }

    /// R80-4-02: a write removes an ID3v2.2 frame that has no ID3v2.4 ID,
    /// and the write result names that frame. The write keeps the other
    /// old values.
    #[test]
    fn adr_0080_old_itunes_frames_unmapped_frame_is_removed_and_named() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("track.mp3");
        let fixture = tagged_fixture(
            2,
            &[
                v22_text_frame(*b"TT2", "Old title"),
                v22_text_frame(*b"TCP", "1"),
                v22_text_frame(*b"XYZ", "unknown"),
            ],
        );
        fs::write(&path, &fixture).expect("write fixture");

        let result = write_id3v24_edits(&path, &old_itunes_edits()).expect("write tags");

        assert_eq!(result.applied, 4);
        assert_eq!(result.removed_frames, vec!["XYZ".to_owned()]);
        let tag = Tag::read_from_path(&path).expect("read written tag");
        assert!(tag.frames().all(|frame| frame.id() != "XYZ"));
        assert_eq!(
            tag.get("TCMP").and_then(|frame| frame.content().text()),
            Some("1")
        );
        let written = fs::read(&path).expect("read written file");
        assert_eq!(audio_after_tag(&written), OLD_ITUNES_AUDIO);
    }

    /// R80-4-05: on a file with an ID3v2.3 or ID3v2.4 tag, the write gives
    /// the same bytes as the earlier write in place.
    #[test]
    fn adr_0080_old_itunes_frames_v23_and_v24_writes_are_unchanged() {
        for major_version in [3, 4] {
            let temp = tempfile::tempdir().expect("temp dir");
            let path = temp.path().join("track.mp3");
            let expected_path = temp.path().join("expected.mp3");
            let text = |value: &str| {
                let mut content = vec![if major_version == 4 { 3 } else { 0 }];
                content.extend_from_slice(value.as_bytes());
                content
            };
            let frame = |id: &[u8; 4], content: &[u8]| {
                let mut frame = v24_frame(*id, content);
                if major_version == 3 {
                    let size = u32::try_from(content.len()).expect("frame size");
                    frame[4..8].copy_from_slice(&size.to_be_bytes());
                }
                frame
            };
            let fixture = tagged_fixture(
                major_version,
                &[
                    frame(b"TIT2", &text("Old title")),
                    frame(b"TPE2", &text("Other tool artist")),
                    frame(b"TSOP", &text("Doerfels")),
                ],
            );
            fs::write(&path, &fixture).expect("write fixture");
            fs::write(&expected_path, &fixture).expect("write expected fixture");
            let edits = old_itunes_edits();

            let result = write_id3v24_edits(&path, &edits).expect("write tags");

            let mut tag = Tag::read_from_path(&expected_path).expect("read fixture tag");
            for edit in &edits {
                tag.add_frame(super::id3v24_edit_frame(edit, &LocalFiles).expect("edit frame"));
            }
            tag.write_to_path(&expected_path, id3::Version::Id3v24)
                .expect("write in place");
            assert_eq!(result.applied, edits.len());
            assert!(result.removed_frames.is_empty());
            assert_eq!(
                fs::read(&path).expect("read written file"),
                fs::read(&expected_path).expect("read expected file"),
                "ID3v2.{major_version}"
            );
        }
    }

    fn wpay_frame_values(tag: &Tag) -> Vec<String> {
        tag.frames()
            .filter(|frame| frame.id() == "WPAY")
            .map(|frame| frame.content().link().unwrap_or_default().to_string())
            .collect()
    }

    /// HitIt ADR 0080 Decision 10: a file with a different `WPAY` value from
    /// another tool holds only the RSS link after one write, in one frame.
    #[test]
    fn adr_0080_funding_write_replaces_a_foreign_wpay_value() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        write_id3v24_edits(
            temp.path(),
            &[Id3v24Edit {
                frame_label: "WPAY".into(),
                value: "https://foreign.example/pay".into(),
            }],
        )
        .expect("seed the foreign value");
        let seeded = Tag::read_from_path(temp.path()).expect("read seeded tag");
        assert_eq!(
            wpay_frame_values(&seeded),
            vec!["https://foreign.example/pay"]
        );

        let rss_link = Id3v24Edit {
            frame_label: "WPAY".into(),
            value: "https://rss.example/tip".into(),
        };
        write_id3v24_edits(temp.path(), std::slice::from_ref(&rss_link)).expect("write");

        let tag = Tag::read_from_path(temp.path()).expect("read written tag");
        assert_eq!(wpay_frame_values(&tag), vec!["https://rss.example/tip"]);

        write_id3v24_edits(temp.path(), &[rss_link]).expect("second write");
        let tag = Tag::read_from_path(temp.path()).expect("read second tag");
        assert_eq!(wpay_frame_values(&tag), vec!["https://rss.example/tip"]);
    }

    /// HitIt ADR 0080 Decision 10: the `add_frame` call of the id3 crate replaces
    /// every earlier `WPAY` frame. A file with two `WPAY` frames from
    /// another tool holds one frame, the RSS link, after one write. The
    /// writer needs no removal step of its own.
    #[test]
    fn adr_0080_funding_write_replaces_two_foreign_wpay_frames() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        let mut tag = Tag::new();
        tag.frames_vec_mut().push(Frame::with_content(
            "WPAY",
            id3::frame::Content::Link("https://a.example/".into()),
        ));
        tag.frames_vec_mut().push(Frame::with_content(
            "WPAY",
            id3::frame::Content::Link("https://b.example/".into()),
        ));
        tag.write_to_path(temp.path(), id3::Version::Id3v24)
            .expect("seed");
        write_id3v24_edits(
            temp.path(),
            &[Id3v24Edit {
                frame_label: "WPAY".into(),
                value: "https://rss.example/tip".into(),
            }],
        )
        .expect("w");
        let tag = Tag::read_from_path(temp.path()).expect("read");
        assert_eq!(wpay_frame_values(&tag), vec!["https://rss.example/tip"]);
    }

    /// HitIt ADR 0080 Decision 10: a file with a `WPAY` value keeps it when the
    /// write holds no `WPAY` edit. The write adds no frame.
    #[test]
    fn adr_0080_funding_write_without_a_link_keeps_the_wpay_value() {
        let temp = tempfile::NamedTempFile::new().expect("temp file");
        fs::write(temp.path(), b"not really an mp3").expect("write file");
        write_id3v24_edits(
            temp.path(),
            &[Id3v24Edit {
                frame_label: "WPAY".into(),
                value: "https://foreign.example/pay".into(),
            }],
        )
        .expect("seed the foreign value");

        write_id3v24_edits(
            temp.path(),
            &[Id3v24Edit {
                frame_label: "TIT2".into(),
                value: "Song".into(),
            }],
        )
        .expect("write a track with no funding link");

        let tag = Tag::read_from_path(temp.path()).expect("read written tag");
        assert_eq!(wpay_frame_values(&tag), vec!["https://foreign.example/pay"]);
    }

    /// HitIt ADR 0080 Decision 13: the language goes into the `LANGUAGE` key of a
    /// Vorbis Comment and into the `LANGUAGE` freeform atom of an MP4 file.
    #[test]
    fn adr_0080_language_frame_maps_to_the_language_key() {
        assert_eq!(
            vorbis_storage_key("TLAN", crate::format::AudioFormat::Flac),
            Some("LANGUAGE".into())
        );
        assert_eq!(
            super::lofty_generic_key("TLAN", lofty::tag::TagType::Mp4Ilst).as_deref(),
            Some("----:com.apple.iTunes:LANGUAGE")
        );
    }

    /// HitIt ADR 0080 Decision 10: the funding link goes into the Vorbis key of
    /// `UrlKind::Payment`.
    #[test]
    fn adr_0080_funding_frame_maps_to_the_payment_key() {
        assert_eq!(
            vorbis_storage_key("WPAY", crate::format::AudioFormat::Flac),
            Some("PAYMENTWEBPAGE".into())
        );
    }
    fn license_edit(frame_label: &str, value: &str) -> Id3v24Edit {
        Id3v24Edit {
            frame_label: frame_label.into(),
            value: value.into(),
        }
    }

    /// HitIt ADR 0080 Decision 11: one URL gives one `WCOP` edit, and more than one
    /// URL gives one `TXXX:LICENSE` edit in the sequence of the edits, with
    /// each URL one time. A write with no license edit replaces nothing.
    #[test]
    fn adr_0080_license_resolution_gives_one_frame() {
        let rss = license_edit("WCOP", "https://rss.example/by");
        let mb = license_edit(
            "TXXX:LICENSE",
            "https://mb.example/one · https://rss.example/by/",
        );

        let one = resolve_license_edits(std::slice::from_ref(&rss));
        assert_eq!(one.edits, vec![rss.clone()]);
        assert!(one.replaces_license);

        let both = resolve_license_edits(&[mb, rss]);
        assert_eq!(
            both.edits,
            vec![license_edit(
                "TXXX:LICENSE",
                "https://mb.example/one · https://rss.example/by/"
            )]
        );
        assert!(both.replaces_license);

        let none = resolve_license_edits(&[license_edit("TLAN", "eng")]);
        assert!(!none.replaces_license);
    }

    /// Musicindex ADR 0093 R-TAG-8 item 2: a test covers each row of the
    /// conversion table. Each converted tag encodes as ID3v2.4, so the content
    /// is valid for its new ID.
    #[test]
    fn adr_0090_each_row_of_the_old_frame_table_converts() {
        use id3::Content;
        use id3::frame::{InvolvedPeopleList, InvolvedPeopleListItem};

        use super::{OLD_FRAME_IDS, OldFrame, convert_old_frames};

        for (old_id, row) in OLD_FRAME_IDS {
            let mut tag = Tag::with_version(id3::Version::Id3v23);
            // A date row gets the date frames before it in the table, so its
            // part of the date is in the merged `TDRC`.
            let date = [("TYER", "2001"), ("TDAT", "2412"), ("TIME", "1305")];
            let date_row = date.iter().position(|(id, _)| *id == old_id);
            for (id, text) in date.iter().take(date_row.map_or(0, |row| row + 1)) {
                tag.add_frame(Frame::text(*id, *text));
            }
            // A file gives `IPLS` the content of a people list.
            let content = if old_id == "IPLS" {
                Content::InvolvedPeopleList(InvolvedPeopleList {
                    items: vec![InvolvedPeopleListItem {
                        involvement: "producer".into(),
                        involvee: "Name".into(),
                    }],
                })
            } else {
                Content::Text("value".into())
            };
            if date_row.is_none() {
                tag.add_frame(Frame::with_content(old_id, content.clone()));
            }

            let changes = convert_old_frames(&mut tag, Some(3));
            assert!(tag.get(old_id).is_none(), "{old_id}");
            match row {
                OldFrame::Rename(new_id) => {
                    assert!(
                        changes.removed.is_empty(),
                        "{old_id}: {:?}",
                        changes.removed
                    );
                    assert_eq!(changes.converted, [old_id], "{old_id}");
                    assert_eq!(
                        tag.get(new_id).map(Frame::content),
                        Some(&content),
                        "{old_id}"
                    );
                }
                OldFrame::MergeIntoTdrc => {
                    let row = date_row.expect("a date row");
                    let expected = ["2001", "2001-12-24", "2001-12-24T13:05"][row];
                    assert!(
                        changes.removed.is_empty(),
                        "{old_id}: {:?}",
                        changes.removed
                    );
                    assert_eq!(changes.converted.len(), row + 1, "{old_id}");
                    assert_eq!(
                        tag.get("TDRC").and_then(|frame| frame.content().text()),
                        Some(expected),
                        "{old_id}"
                    );
                }
                OldFrame::Remove => assert_eq!(changes.removed, [old_id], "{old_id}"),
            }
            assert_eq!(
                tag.frames().count(),
                usize::from(!matches!(row, OldFrame::Remove))
            );
            tag.write_to(std::io::sink(), id3::Version::Id3v24)
                .unwrap_or_else(|error| panic!("{old_id}: {error}"));
        }
    }

    /// R-TAG-9 item 1: a date part that is not a valid date or time stops
    /// the merge, and the write removes the frame of that part.
    #[test]
    fn adr_0090_a_date_part_that_is_not_valid_stops_the_merge() {
        use super::convert_old_frames;

        for (day_month, time, expected, removed) in [
            ("3213", "1305", "2001", vec!["TDAT", "TIME"]),
            ("ab12", "1305", "2001", vec!["TDAT", "TIME"]),
            ("2412", "2599", "2001-12-24", vec!["TIME"]),
        ] {
            let mut tag = Tag::with_version(id3::Version::Id3v23);
            tag.add_frame(Frame::text("TYER", "2001"));
            tag.add_frame(Frame::text("TDAT", day_month));
            tag.add_frame(Frame::text("TIME", time));
            let changes = convert_old_frames(&mut tag, Some(3));
            assert_eq!(
                tag.get("TDRC").and_then(|frame| frame.content().text()),
                Some(expected)
            );
            assert_eq!(changes.removed, removed, "{day_month} {time}");
        }
    }
}
