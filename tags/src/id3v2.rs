//! The ID3v2 frame loop of musicindex ADR 0090 R-TAG-7.
//!
//! The `id3` crate stops at the first bad frame. This loop finds the bounds
//! of each frame itself. It gives each frame to `id3` alone, so a bad frame
//! does not stop the read. The read keeps each good frame before and after a
//! bad frame and returns a list of the bad frames.

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};
use id3::{Content, Frame, Tag, TagLike, Version};

use crate::frames;

/// The length of the ID3v2 tag header and of the ID3v2.4 footer.
const HEADER_LEN: usize = 10;

/// The length of an ID3v1 tag at the end of a file.
const ID3V1_LEN: u64 = 128;

/// The ID3v2.4 tag flag for a footer.
const FOOTER_FLAG: u8 = 0x10;
/// The tag flag for an extended header in ID3v2.3 and ID3v2.4.
const EXTENDED_HEADER_FLAG: u8 = 0x40;
/// The tag flag for unsynchronisation in each version.
const UNSYNCHRONISATION_FLAG: u8 = 0x80;
/// The ID3v2.2 tag flag for compression. No reader knows this compression.
const V22_COMPRESSION_FLAG: u8 = 0x40;

/// The ID3v2.3 frame flags.
const V23_COMPRESSION: u16 = 0x0080;
const V23_ENCRYPTION: u16 = 0x0040;
/// The ID3v2.4 frame flags.
const V24_COMPRESSION: u16 = 0x0008;
const V24_ENCRYPTION: u16 = 0x0004;
const V24_UNSYNCHRONISATION: u16 = 0x0002;
const V24_DATA_LENGTH_INDICATOR: u16 = 0x0001;

/// A frame that the frame loop cannot read (R-TAG-7 item 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadFrame {
    /// The offset of the frame header from the start of the file. In an
    /// ID3v2.2 or ID3v2.3 tag with unsynchronisation, the offset is in the
    /// tag after the loop removes the unsynchronisation.
    pub position: usize,
    /// The frame ID, when it is readable.
    pub id: Option<String>,
    /// Why the frame is bad.
    pub cause: String,
    /// The label of the suite frame (R-TAG-2, R-TAG-4) when the ID and the
    /// description read correctly, for example `TXXX:Podcast Feed Guid`
    /// (R-TAG-9 item 2). A write removes the frame and writes it again from
    /// the data of the app (R-TAG-9 item 3).
    pub suite_label: Option<String>,
    /// The entry of the list of known bad frames (R-TAG-9 item 4), or `None`.
    pub known: Option<KnownBadFrame>,
}

/// The list of known bad frames of musicindex ADR 0090 R-TAG-9 item 4. Each
/// entry has a test file with the bad frame in `tests/id3v2.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownBadFrame {
    /// A frame with a size of 0. It holds no data, so a repair is not
    /// possible (R-TAG-9 item 5).
    ZeroSize,
    /// A `TLEN` frame whose text is not a positive whole number of
    /// milliseconds (musicindex ADR 0091 R-TAG-11 item 4). The write removes
    /// it and writes the measured length.
    LengthNotAWholeNumber,
}

/// What a write does with a known bad frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadFrameAction {
    /// The write repairs the frame and keeps each byte of the content.
    Repair,
    /// The write removes the frame.
    Remove,
}

impl KnownBadFrame {
    /// Each entry of the list.
    pub const ALL: [Self; 2] = [Self::ZeroSize, Self::LengthNotAWholeNumber];

    /// The action of a write for this entry.
    #[must_use]
    pub const fn action(self) -> BadFrameAction {
        match self {
            Self::ZeroSize | Self::LengthNotAWholeNumber => BadFrameAction::Remove,
        }
    }
}

/// The problems that a read found in an ID3v2 tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagProblems {
    /// The cause when the tag header is bad (R-TAG-7 item 5). Then the read
    /// gives no frame, and a write does not change the file.
    pub bad_tag: Option<String>,
    /// The bad frames, in the sequence of the tag.
    pub bad_frames: Vec<BadFrame>,
}

impl TagProblems {
    /// True when the read found no problem.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bad_tag.is_none() && self.bad_frames.is_empty()
    }
}

/// The error of a write to a file with a bad ID3v2 tag header (R-TAG-7 item
/// 5). The write does not change the file, because it cannot find the end
/// of the old tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadTag {
    /// Why the tag header is bad.
    pub cause: String,
}

impl std::fmt::Display for BadTag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the ID3v2 tag header is bad ({}), so the write does not change the file",
            self.cause
        )
    }
}

impl std::error::Error for BadTag {}

/// The result of the frame loop for one file.
#[derive(Clone, Debug, Default)]
pub struct Id3v2Read {
    /// The major version of the ID3v2 tag: 2, 3 or 4. `None` when the file
    /// has no ID3v2 tag.
    pub version: Option<u8>,
    /// The good frames. The `id3` crate gives an ID3v2.2 frame its ID3v2.3 ID
    /// when it knows one.
    pub tag: Tag,
    /// The bad tag header or the bad frames.
    pub problems: TagProblems,
    /// True when the file ends with an ID3v1 tag.
    pub has_id3v1: bool,
    /// The length in bytes of the ID3v2 tag at the start of the file, with
    /// its header and its footer. 0 when the file has no tag or a bad tag.
    pub tag_len: u64,
    /// True when an ID3v2.4 frame size was a plain number, not a syncsafe
    /// number (R-TAG-7 item 4). The tag is not correct, so a write repairs
    /// it.
    pub plain_frame_sizes: bool,
}

/// Reads the ID3v2 tag at the start of `path` with the frame loop of
/// R-TAG-7.
///
/// # Errors
///
/// Returns an error when the file cannot be read. A bad tag or a bad frame
/// is not an error. [`Id3v2Read::problems`] gives it.
pub fn read_id3v2(path: &Path) -> Result<Id3v2Read> {
    let mut file =
        File::open(path).with_context(|| format!("open {} for its ID3 tag", path.display()))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("read the length of {}", path.display()))?
        .len();
    let has_id3v1 = has_id3v1(&mut file, file_len)
        .with_context(|| format!("look for an ID3v1 tag in {}", path.display()))?;

    let mut read = Id3v2Read {
        has_id3v1,
        ..Id3v2Read::default()
    };
    let mut header = [0u8; HEADER_LEN];
    if file_len < HEADER_LEN as u64 {
        return Ok(read);
    }
    file.read_exact(&mut header)
        .with_context(|| format!("read the ID3 header of {}", path.display()))?;
    if &header[..3] != b"ID3" {
        return Ok(read);
    }
    let header = match TagHeader::parse(&header, file_len) {
        Ok(header) => header,
        Err(cause) => {
            read.version = Some(header[3]);
            read.problems.bad_tag = Some(cause);
            return Ok(read);
        }
    };
    let mut body = vec![0u8; header.size];
    file.read_exact(&mut body)
        .with_context(|| format!("read the ID3 tag of {}", path.display()))?;

    read.version = Some(header.major);
    match parse_frames(&header, body) {
        Ok((frames, bad_frames, plain_sizes)) => {
            read.tag = Tag::with_version(match header.major {
                2 => Version::Id3v22,
                3 => Version::Id3v23,
                _ => Version::Id3v24,
            });
            for frame in frames {
                read.tag.add_frame(frame);
            }
            read.problems.bad_frames = bad_frames;
            read.plain_frame_sizes = plain_sizes;
            read.tag_len = header.total_len();
        }
        Err(cause) => read.problems.bad_tag = Some(cause),
    }
    Ok(read)
}

/// True when the last 128 bytes of the file start with `TAG`. The read
/// position goes back to the start of the file.
fn has_id3v1(file: &mut File, file_len: u64) -> std::io::Result<bool> {
    let mut marker = [0u8; 3];
    let found = if file_len >= ID3V1_LEN {
        file.seek(SeekFrom::Start(file_len - ID3V1_LEN))?;
        file.read_exact(&mut marker)?;
        &marker == b"TAG"
    } else {
        false
    };
    file.seek(SeekFrom::Start(0))?;
    Ok(found)
}

/// The fields of a good ID3v2 tag header.
struct TagHeader {
    major: u8,
    flags: u8,
    /// The size of the tag after the header, without a footer.
    size: usize,
}

impl TagHeader {
    /// Parses the 10 header bytes. It gives the cause when the header is bad.
    fn parse(bytes: &[u8; HEADER_LEN], file_len: u64) -> Result<Self, String> {
        let major = bytes[3];
        if !(2..=4).contains(&major) || bytes[4] == 0xFF {
            return Err(format!(
                "the tag version 2.{major}.{} is not known",
                bytes[4]
            ));
        }
        let flags = bytes[5];
        if major == 2 && flags & V22_COMPRESSION_FLAG != 0 {
            return Err("the ID3v2.2 tag is compressed".to_owned());
        }
        let size = syncsafe(&bytes[6..10])
            .ok_or_else(|| "the tag size is not a syncsafe number".to_owned())?;
        let header = Self { major, flags, size };
        if header.total_len() > file_len {
            return Err(format!(
                "the tag size {} goes past the end of the file",
                header.size
            ));
        }
        Ok(header)
    }

    fn total_len(&self) -> u64 {
        let footer = if self.major == 4 && self.flags & FOOTER_FLAG != 0 {
            HEADER_LEN
        } else {
            0
        };
        (HEADER_LEN + self.size + footer) as u64
    }
}

/// The good frames, the bad frames, and true when an ID3v2.4 frame size
/// was a plain number.
type Frames = (Vec<Frame>, Vec<BadFrame>, bool);

/// The frame loop of R-TAG-7 item 4. It gives the cause when the extended
/// header is bad, because then the start of the frames is not known.
fn parse_frames(header: &TagHeader, mut body: Vec<u8>) -> Result<Frames, String> {
    let tag_unsync = header.flags & UNSYNCHRONISATION_FLAG != 0;
    if tag_unsync && header.major < 4 {
        body = resync(&body);
    }
    let mut pos = 0;
    if header.major >= 3 && header.flags & EXTENDED_HEADER_FLAG != 0 {
        let size_bytes = body
            .get(..4)
            .ok_or_else(|| "the extended header is past the end of the tag".to_owned())?;
        // ID3v2.3: the size does not count its 4 bytes, and is 6 or 10.
        // ID3v2.4: the syncsafe size counts each byte, and is 6 or more.
        let extended_len = if header.major == 3 {
            Some(be_u32(size_bytes))
                .filter(|size| *size == 6 || *size == 10)
                .map(|size| size + 4)
        } else {
            syncsafe(size_bytes).filter(|size| *size >= 6)
        }
        .ok_or_else(|| "the extended header size is not valid".to_owned())?;
        if extended_len > body.len() {
            return Err("the extended header is past the end of the tag".to_owned());
        }
        pos = extended_len;
    }

    let layout = FrameLayout::of(header.major);
    let mut frames = Vec::new();
    let mut bad_frames = Vec::new();
    let mut plain_sizes = false;
    while pos + layout.header_len <= body.len() {
        // R-TAG-7 item 4: a zero byte starts the padding.
        if body[pos] == 0 {
            break;
        }
        let frame_header = &body[pos..pos + layout.header_len];
        let id_bytes = &frame_header[..layout.id_len];
        let id = is_frame_id(id_bytes).then(|| String::from_utf8_lossy(id_bytes).into_owned());
        let (size, plain) = layout.frame_size(&body, pos);
        plain_sizes |= plain;
        let bad = |cause: String, known: Option<KnownBadFrame>, suite_label| BadFrame {
            position: HEADER_LEN + pos,
            id: id.clone(),
            cause,
            suite_label,
            known,
        };
        let Some(end) = (pos + layout.header_len)
            .checked_add(size)
            .filter(|end| *end <= body.len())
        else {
            bad_frames.push(bad(
                format!("the frame size {size} goes past the end of the tag"),
                None,
                None,
            ));
            break;
        };
        let data = &body[pos + layout.header_len..end];
        let flags = layout.flags(frame_header);
        let result = match &id {
            None => Err(bad("the frame ID is not valid".to_owned(), None, None)),
            Some(_) if size == 0 => Err(bad(
                "the frame size is 0".to_owned(),
                Some(KnownBadFrame::ZeroSize),
                None,
            )),
            Some(_) if layout.is_encrypted(flags) => {
                Err(bad("the frame is encrypted".to_owned(), None, None))
            }
            Some(id) => {
                decode_frame(header.major, frame_header, data, tag_unsync).map_err(|cause| {
                    let label = suite_label(header.major, id, flags, data, tag_unsync);
                    bad(cause, None, label)
                })
            }
        };
        let result = result.and_then(|frame| {
            if frame.id() == "TLEN" && length_ms(&frame).is_none() {
                Err(bad(
                    "the TLEN text is not a positive whole number".to_owned(),
                    Some(KnownBadFrame::LengthNotAWholeNumber),
                    None,
                ))
            } else {
                Ok(frame)
            }
        });
        match result {
            Ok(frame) => frames.push(frame),
            Err(bad_frame) => bad_frames.push(bad_frame),
        }
        pos = end;
    }
    Ok((frames, bad_frames, plain_sizes))
}

/// The length in milliseconds of a `TLEN` frame, when its text is a positive
/// whole number (musicindex ADR 0091 R-TAG-11 item 4).
pub(crate) fn length_ms(frame: &Frame) -> Option<u64> {
    let text = frame.content().text()?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok().filter(|length| *length > 0)
}

/// The frame header layout of one tag version.
struct FrameLayout {
    major: u8,
    header_len: usize,
    id_len: usize,
}

impl FrameLayout {
    fn of(major: u8) -> Self {
        if major == 2 {
            Self {
                major,
                header_len: 6,
                id_len: 3,
            }
        } else {
            Self {
                major,
                header_len: 10,
                id_len: 4,
            }
        }
    }

    fn flags(&self, frame_header: &[u8]) -> u16 {
        if self.major == 2 {
            0
        } else {
            u16::from_be_bytes([frame_header[8], frame_header[9]])
        }
    }

    fn is_encrypted(&self, flags: u16) -> bool {
        match self.major {
            3 => flags & V23_ENCRYPTION != 0,
            4 => flags & V24_ENCRYPTION != 0,
            _ => false,
        }
    }

    /// The size of the frame at `pos` (R-TAG-7 item 4). ID3v2.3 has a plain
    /// size and ID3v2.4 has a syncsafe size. When the ID3v2.4 size points to
    /// no frame header, the plain reading is used if it points to a frame
    /// header or to the end of the frames.
    ///
    /// The flag is true when the loop used a plain ID3v2.4 size that is not
    /// equal to the syncsafe reading.
    fn frame_size(&self, body: &[u8], pos: usize) -> (usize, bool) {
        match self.major {
            2 => {
                let bytes = &body[pos + 3..pos + 6];
                let size = (usize::from(bytes[0]) << 16)
                    | (usize::from(bytes[1]) << 8)
                    | usize::from(bytes[2]);
                (size, false)
            }
            3 => (be_u32(&body[pos + 4..pos + 8]), false),
            _ => {
                let bytes = &body[pos + 4..pos + 8];
                let plain = be_u32(bytes);
                let next = |size: usize| pos.saturating_add(self.header_len).saturating_add(size);
                let points =
                    |next: usize| points_to_frame(body, next, self.id_len, self.header_len);
                let synced = syncsafe(bytes);
                match synced {
                    Some(size) if points(next(size)) => (size, false),
                    _ if points(next(plain)) => (plain, synced != Some(plain)),
                    Some(size) => (size, false),
                    None => (plain, true),
                }
            }
        }
    }
}

/// True when `next` is the end of the frames, the start of the padding, or
/// the start of a valid ID3v2.4 frame header. Padding is zero bytes to the
/// end of the frames. A valid frame header has a valid ID and a size, in
/// the syncsafe or the plain reading, that stays in the frames.
fn points_to_frame(body: &[u8], next: usize, id_len: usize, header_len: usize) -> bool {
    let Some(rest) = body.get(next..) else {
        return false;
    };
    if rest.iter().all(|byte| *byte == 0) {
        return true;
    }
    let Some(header) = rest.get(..header_len) else {
        return false;
    };
    let room = rest.len() - header_len;
    let size = &header[id_len..id_len + 4];
    is_frame_id(&header[..id_len])
        && (syncsafe(size).is_some_and(|size| size <= room) || be_u32(size) <= room)
}

/// True when each byte is an upper case ASCII letter or a digit.
fn is_frame_id(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

/// Decodes one frame with the `id3` crate. The frame goes into a tag of its
/// own, so an error in this frame does not stop the loop.
fn decode_frame(
    major: u8,
    frame_header: &[u8],
    data: &[u8],
    tag_unsync: bool,
) -> Result<Frame, String> {
    let mut frame_header = frame_header.to_vec();
    let mut data = data.to_vec();
    if major == 4 {
        let mut flags = u16::from_be_bytes([frame_header[8], frame_header[9]]);
        if tag_unsync || flags & V24_UNSYNCHRONISATION != 0 {
            data = resync(&data);
            flags &= !V24_UNSYNCHRONISATION;
        }
        let size = to_syncsafe(data.len()).ok_or_else(|| "the frame is too large".to_owned())?;
        frame_header[4..8].copy_from_slice(&size);
        frame_header[8..10].copy_from_slice(&flags.to_be_bytes());
    }
    let size = to_syncsafe(frame_header.len() + data.len())
        .ok_or_else(|| "the frame is too large".to_owned())?;
    let mut single = Vec::with_capacity(HEADER_LEN + frame_header.len() + data.len());
    single.extend_from_slice(b"ID3");
    single.extend_from_slice(&[major, 0, 0]);
    single.extend_from_slice(&size);
    single.extend_from_slice(&frame_header);
    single.extend_from_slice(&data);
    let tag = Tag::read_from2(Cursor::new(single)).map_err(|err| err.to_string())?;
    let frame = tag
        .frames()
        .next()
        .cloned()
        .ok_or_else(|| "the frame gives no content".to_owned())?;
    Ok(if major < 4 {
        join_slash_values(frame)
    } else {
        frame
    })
}

/// The ID3v2.2 and ID3v2.3 frames whose "/" the `id3` crate changes to a
/// NUL, with their ID3v2.3 IDs. The `id3` crate gives an ID3v2.2 frame its
/// ID3v2.3 ID.
const SLASH_VALUE_FRAMES: [&str; 5] = ["TCOM", "TEXT", "TOLY", "TOPE", "TPE1"];

/// Gives the text of a frame of [`SLASH_VALUE_FRAMES`] its "/" again.
///
/// Orchestrator choice of 2026-11-22 under musicindex ADR 0090 R-TAG-9 item
/// 1: a "/" can be a separator or a part of a name, for example "AC/DC". The
/// division of the value is thus not sure, so the read keeps one value with
/// each byte of the text. In ID3v2.2 and ID3v2.3, the `id3` crate stops the
/// text at the first NUL of the frame. Thus each NUL in the text comes from a
/// "/", and the change back keeps each byte.
fn join_slash_values(frame: Frame) -> Frame {
    if !SLASH_VALUE_FRAMES.contains(&frame.id()) {
        return frame;
    }
    match frame.content().text() {
        Some(text) if text.contains('\0') => {
            Frame::with_content(frame.id(), Content::Text(text.replace('\0', "/")))
        }
        _ => frame,
    }
}

/// The suite label of a bad frame when its ID and its description read
/// correctly (R-TAG-9 item 2).
fn suite_label(major: u8, id: &str, flags: u16, data: &[u8], tag_unsync: bool) -> Option<String> {
    if matches!(id, "CHAP" | "CTOC") {
        return Some(id.to_owned());
    }
    let description_start = match id {
        "TXXX" => 1,
        "COMM" | "USLT" => 4,
        "SYLT" => 6,
        _ => return None,
    };
    let content = match major {
        3 if flags & V23_COMPRESSION != 0 => return None,
        4 if flags & V24_COMPRESSION != 0 => return None,
        4 => {
            let content = if tag_unsync || flags & V24_UNSYNCHRONISATION != 0 {
                resync(data)
            } else {
                data.to_vec()
            };
            if flags & V24_DATA_LENGTH_INDICATOR != 0 {
                content.get(4..)?.to_vec()
            } else {
                content
            }
        }
        _ => data.to_vec(),
    };
    let encoding = *content.first()?;
    let description = terminated_text(encoding, content.get(description_start..)?)?;
    let label = format!("{id}:{description}");
    frames::PODCAST_FRAMES
        .iter()
        .find(|frame| frame.eq_ignore_ascii_case(&label))
        .map(|frame| (*frame).to_owned())
}

/// The text before the first terminator in `bytes`, or `None` when the text
/// has no terminator or does not decode.
fn terminated_text(encoding: u8, bytes: &[u8]) -> Option<String> {
    match encoding {
        0 => {
            let end = bytes.iter().position(|byte| *byte == 0)?;
            Some(bytes[..end].iter().copied().map(char::from).collect())
        }
        3 => {
            let end = bytes.iter().position(|byte| *byte == 0)?;
            std::str::from_utf8(&bytes[..end]).ok().map(str::to_owned)
        }
        1 | 2 => {
            let end = bytes.chunks_exact(2).position(|unit| unit == [0, 0])? * 2;
            let text = &bytes[..end];
            let (big_endian, text) = match (encoding, text) {
                (2, text) => (true, text),
                (_, [0xFE, 0xFF, rest @ ..]) => (true, rest),
                (_, [0xFF, 0xFE, rest @ ..]) => (false, rest),
                _ => return None,
            };
            let units = text
                .chunks_exact(2)
                .map(|unit| {
                    if big_endian {
                        u16::from_be_bytes([unit[0], unit[1]])
                    } else {
                        u16::from_le_bytes([unit[0], unit[1]])
                    }
                })
                .collect::<Vec<_>>();
            String::from_utf16(&units).ok()
        }
        _ => None,
    }
}

/// Removes the unsynchronisation: each `0x00` after `0xFF` goes.
fn resync(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut after_ff = false;
    for &byte in bytes {
        if !(after_ff && byte == 0) {
            out.push(byte);
        }
        after_ff = byte == 0xFF;
    }
    out
}

fn be_u32(bytes: &[u8]) -> usize {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize
}

/// The value of 4 syncsafe bytes, or `None` when a byte has its high bit set.
fn syncsafe(bytes: &[u8]) -> Option<usize> {
    bytes.iter().try_fold(0usize, |value, &byte| {
        (byte & 0x80 == 0).then_some((value << 7) | usize::from(byte))
    })
}

fn to_syncsafe(value: usize) -> Option<[u8; 4]> {
    (value < 1 << 28).then(|| {
        [21, 14, 7, 0].map(|shift| u8::try_from((value >> shift) & 0x7F).unwrap_or_default())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resync_removes_each_zero_after_ff() {
        assert_eq!(
            resync(&[0xFF, 0x00, 0xE0, 0xFF, 0x00, 0x00]),
            [0xFF, 0xE0, 0xFF, 0x00]
        );
    }

    #[test]
    fn syncsafe_round_trip() {
        assert_eq!(syncsafe(&to_syncsafe(1014).expect("small")), Some(1014));
        assert_eq!(syncsafe(&[0, 0, 0x80, 0]), None);
    }

    #[test]
    fn terminated_text_reads_each_encoding() {
        assert_eq!(
            terminated_text(0, b"Podcast\0x").as_deref(),
            Some("Podcast")
        );
        assert_eq!(terminated_text(3, b"Podcast\0").as_deref(), Some("Podcast"));
        assert_eq!(
            terminated_text(1, &[0xFF, 0xFE, b'P', 0, 0, 0]).as_deref(),
            Some("P")
        );
        assert_eq!(terminated_text(2, &[0, b'P', 0, 0]).as_deref(), Some("P"));
        assert_eq!(terminated_text(0, b"no terminator"), None);
        assert_eq!(terminated_text(1, &[b'P', 0, 0, 0]), None);
    }

    #[test]
    fn each_known_bad_frame_has_an_action() {
        for entry in KnownBadFrame::ALL {
            assert_eq!(entry.action(), BadFrameAction::Remove);
        }
    }
}
