//! The transcript parser of musicindex ADR 0103 R-LYRIC-3.
//!
//! [`parse`] reads a transcript file or a lyrics file and gives its cues. The
//! tag crate `musicindex-tags` uses the cues for its `SYLT` frame. A lyrics
//! index serves them as namespace JSON with [`Transcript::to_namespace`]
//! (musicindex ADR 0103 R-LYRIC-2 and musicindex ADR 0104 R-LYRIC-2 item 3).
//!
//! The crate has no dependency on `id3` or `lofty`, so a service can use it
//! without the tag crate (musicindex ADR 0104 R-LYRIC-9 item 1).
//!
//! The parser reads SRT, VTT, LRC, SUB, the namespace JSON and
//! plain text. It removes a BOM, puts the cues in sequence by start time, and
//! stops a cue at the start of the subsequent cue when the two overlap.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The `version` of the namespace JSON that [`Transcript::to_namespace`] gives.
pub const NAMESPACE_VERSION: &str = "1.0.0";

/// One timed cue of a transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    /// The start time in milliseconds.
    pub start_ms: u32,
    /// The end time in milliseconds. A cue with no end time in the file ends
    /// at the start of the subsequent cue. The last cue can have no end time.
    pub end_ms: Option<u32>,
    /// The text of the cue, as the file gives it.
    pub body: String,
    /// The speaker of the cue, when the file gives one.
    pub speaker: Option<String>,
    /// The number of the line of the cue, from 0. Each cue of one line has the
    /// same number, and the number does not decrease in the sequence of the
    /// cues (musicindex ADR 0104 R-LYRIC-2 item 3).
    pub line: u32,
}

/// The result of [`parse`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transcript {
    /// The timed cues in sequence by start time. A file with no time gives no
    /// cue.
    pub cues: Vec<Cue>,
    /// The text with no time: one row for each line.
    pub plain_text: String,
}

/// The error of [`parse`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    /// The file holds no text.
    Empty,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("transcript is empty"),
        }
    }
}

impl std::error::Error for ParseError {}

/// The namespace JSON form of a transcript (musicindex ADR 0103 R-LYRIC-2).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NamespaceTranscript {
    /// Always [`NAMESPACE_VERSION`].
    pub version: &'static str,
    /// One segment for each cue.
    pub segments: Vec<NamespaceSegment>,
}

/// One segment of [`NamespaceTranscript`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceSegment {
    /// The speaker, when the file gives one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// The start time in seconds.
    pub start_time: f64,
    /// The end time in seconds.
    pub end_time: f64,
    /// The text of the segment.
    pub body: String,
    /// The line number of musicindex ADR 0104 R-LYRIC-2 item 3.
    pub line: u32,
}

impl Transcript {
    /// The namespace JSON form of the cues. A transcript with no time gives no
    /// segment.
    ///
    /// Each segment has `endTime` (musicindex ADR 0103 R-LYRIC-2 item 1). Only
    /// the last cue can have no end time in the file. Its end is
    /// `audio_end_ms` when the caller knows the end of the audio and it is not
    /// before the start of the cue, and else the start of the cue.
    #[must_use]
    pub fn to_namespace(&self, audio_end_ms: Option<u32>) -> NamespaceTranscript {
        NamespaceTranscript {
            version: NAMESPACE_VERSION,
            segments: self
                .cues
                .iter()
                .map(|cue| NamespaceSegment {
                    speaker: cue.speaker.clone(),
                    start_time: f64::from(cue.start_ms) / 1000.0,
                    end_time: f64::from(
                        cue.end_ms
                            .or(audio_end_ms.filter(|end| *end >= cue.start_ms))
                            .unwrap_or(cue.start_ms),
                    ) / 1000.0,
                    body: cue.body.clone(),
                    line: cue.line,
                })
                .collect(),
        }
    }
}

/// Parses the bytes of a transcript file. A UTF-8 BOM or a UTF-16 BOM selects
/// the text encoding, and else the bytes are UTF-8. Then [`parse`] reads the
/// text.
///
/// # Errors
///
/// [`ParseError::Empty`] when the file holds no text.
pub fn parse_bytes(bytes: &[u8], mime_type: Option<&str>) -> Result<Transcript, ParseError> {
    parse(&decode(bytes), mime_type)
}

/// Parses the text of a transcript file.
///
/// `mime_type` is the type that the feed gives for the file, if any. A known
/// type selects the first format to try. Then the parser tries the namespace
/// JSON, SRT or VTT, LRC and SUB in this sequence. A file in
/// none of these formats is plain text: the parser removes the time codes and
/// gives no cue.
///
/// # Errors
///
/// [`ParseError::Empty`] when the file holds no text.
pub fn parse(text: &str, mime_type: Option<&str>) -> Result<Transcript, ParseError> {
    let text = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n");
    let mut raw = Vec::new();
    let hinted = mime_type.and_then(format_from_type);
    for format in hinted.into_iter().chain(Format::SEQUENCE) {
        raw = format.parse(&text);
        if !raw.is_empty() {
            break;
        }
    }
    if raw.is_empty() {
        let plain_text = strip_timecode_data(&text);
        if plain_text.trim().is_empty() {
            return Err(ParseError::Empty);
        }
        return Ok(Transcript {
            cues: Vec::new(),
            plain_text,
        });
    }
    Ok(finish(raw))
}

/// A format with time codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    NamespaceJson,
    SubRipOrWebVtt,
    Lrc,
    MicroDvd,
}

impl Format {
    /// The sequence in which [`parse`] tries the formats.
    const SEQUENCE: [Self; 4] = [
        Self::NamespaceJson,
        Self::SubRipOrWebVtt,
        Self::Lrc,
        Self::MicroDvd,
    ];

    fn parse(self, text: &str) -> Vec<RawCue> {
        match self {
            Self::NamespaceJson => parse_namespace_json(text),
            Self::SubRipOrWebVtt => parse_srt_or_vtt(text),
            Self::Lrc => parse_lrc(text),
            Self::MicroDvd => parse_microdvd_sub(text),
        }
    }
}

/// The format of a MIME type. `application/srt` and `application/x-subrip`
/// are SRT (musicindex ADR 0103 R-LYRIC-3 item 4).
fn format_from_type(mime_type: &str) -> Option<Format> {
    let essence = mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "application/json" => Some(Format::NamespaceJson),
        "application/srt" | "application/x-subrip" | "text/vtt" => Some(Format::SubRipOrWebVtt),
        _ => None,
    }
}

/// A cue before the sort. Cues with the same `group` are on one line.
#[derive(Debug)]
struct RawCue {
    start_ms: u32,
    end_ms: Option<u32>,
    body: String,
    speaker: Option<String>,
    group: u64,
}

impl RawCue {
    fn line_of_its_own(start_ms: u32, end_ms: Option<u32>, body: String, group: usize) -> Self {
        Self {
            start_ms,
            end_ms,
            body,
            speaker: None,
            group: u64::try_from(group).unwrap_or(u64::MAX),
        }
    }
}

/// Sorts the cues by start time, sets the end times, and gives the line
/// numbers and the plain text.
fn finish(mut raw: Vec<RawCue>) -> Transcript {
    raw.sort_by_key(|cue| cue.start_ms);
    let next_starts = raw
        .iter()
        .skip(1)
        .map(|cue| Some(cue.start_ms))
        .chain([None])
        .collect::<Vec<_>>();
    let mut cues = Vec::with_capacity(raw.len());
    let mut line = 0_u32;
    let mut previous_group = None;
    for (cue, next_start) in raw.into_iter().zip(next_starts) {
        let end_ms = match (cue.end_ms, next_start) {
            (Some(end), Some(next)) => Some(end.min(next)),
            (None, Some(next)) => Some(next),
            (end, None) => end,
        };
        if previous_group.is_some_and(|group| group != cue.group) {
            line = line.saturating_add(1);
        }
        previous_group = Some(cue.group);
        cues.push(Cue {
            start_ms: cue.start_ms,
            end_ms,
            body: cue.body,
            speaker: cue.speaker,
            line,
        });
    }
    let plain_text = plain_text_of(&cues);
    Transcript { cues, plain_text }
}

/// One row for each line. The bodies of one line are joined with a space.
fn plain_text_of(cues: &[Cue]) -> String {
    let mut rows: Vec<Vec<&str>> = Vec::new();
    let mut current_line = None;
    for cue in cues {
        if current_line != Some(cue.line) {
            rows.push(Vec::new());
            current_line = Some(cue.line);
        }
        if let Some(row) = rows.last_mut() {
            row.push(cue.body.trim());
        }
    }
    rows.iter()
        .map(|row| row.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The JSON object of a namespace transcript.
#[derive(Deserialize)]
struct NamespaceJson {
    segments: Vec<serde_json::Value>,
}

/// One segment of a namespace transcript. The parser ignores a field that it
/// does not know.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonSegment {
    start_time: f64,
    end_time: Option<f64>,
    body: String,
    speaker: Option<String>,
    line: Option<u64>,
}

/// Reads the namespace JSON (musicindex ADR 0103 R-LYRIC-3 item 5). A segment
/// that the parser cannot read is ignored.
///
/// The line of each segment comes from the first of these that applies:
///
/// 1. The field `line`, when each segment has it and it does not decrease.
/// 2. Else the segments of one word make one line, as in the namespace
///    example. A body that ends with a line break ends its line. A change of
///    speaker starts a line. A body of more than one word is a full line, and
///    it gets its own line (musicindex ADR 0104 R-LYRIC-2 item 3, point 3).
fn parse_namespace_json(text: &str) -> Vec<RawCue> {
    if !text.trim_start().starts_with('{') {
        return Vec::new();
    }
    let Ok(document) = serde_json::from_str::<NamespaceJson>(text) else {
        return Vec::new();
    };
    let segments = document
        .segments
        .into_iter()
        .filter_map(|segment| serde_json::from_value::<JsonSegment>(segment).ok())
        .collect::<Vec<_>>();
    let line_field = segments.iter().all(|segment| segment.line.is_some())
        && segments.windows(2).all(|pair| pair[0].line <= pair[1].line);

    let mut cues = Vec::new();
    let mut group = 0_u64;
    let mut break_before = false;
    let mut previous_full_line = false;
    let mut previous_speaker: Option<Option<String>> = None;
    for segment in segments {
        let ends_line = ends_with_line_break(&segment.body);
        let Some(start_ms) = seconds_to_ms(segment.start_time) else {
            break_before |= ends_line;
            continue;
        };
        if segment.body.trim().is_empty() {
            break_before |= ends_line;
            continue;
        }
        let key = if line_field {
            segment.line.unwrap_or_default()
        } else {
            let full_line = segment.body.trim().contains(char::is_whitespace);
            if previous_speaker.as_ref().is_some_and(|speaker| {
                break_before || full_line || previous_full_line || *speaker != segment.speaker
            }) {
                group = group.saturating_add(1);
            }
            break_before = ends_line;
            previous_full_line = full_line;
            previous_speaker = Some(segment.speaker.clone());
            group
        };
        cues.push(RawCue {
            start_ms,
            end_ms: segment.end_time.and_then(seconds_to_ms),
            body: segment.body,
            speaker: segment.speaker,
            group: key,
        });
    }
    cues
}

fn ends_with_line_break(body: &str) -> bool {
    body.trim_end_matches([' ', '\t']).ends_with('\n')
}

/// Milliseconds from seconds, or `None` for a value that is not finite, is
/// less than 0 or is too large.
fn seconds_to_ms(seconds: f64) -> Option<u32> {
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let millis = (seconds * 1000.0).round();
    if millis > f64::from(u32::MAX) {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is finite, not negative and not more than u32::MAX"
    )]
    let millis = millis as u32;
    Some(millis)
}

/// Reads SRT or VTT. Each cue is a line.
fn parse_srt_or_vtt(text: &str) -> Vec<RawCue> {
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines = block
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && *line != "WEBVTT")
            .collect::<Vec<_>>();
        let Some(timestamp_index) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let mut times = lines[timestamp_index].split("-->");
        let Some(start) = times.next().and_then(parse_timecode_ms) else {
            continue;
        };
        let end = times.next().and_then(parse_timecode_ms);
        let body = lines
            .iter()
            .skip(timestamp_index + 1)
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        if !body.is_empty() {
            let group = cues.len();
            cues.push(RawCue::line_of_its_own(start, end, body, group));
        }
    }
    cues
}

/// Reads LRC. A row with more than one time gives one cue for each time.
fn parse_lrc(text: &str) -> Vec<RawCue> {
    let mut cues = Vec::new();
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
        for start in starts {
            let group = cues.len();
            cues.push(RawCue::line_of_its_own(start, None, rest.to_owned(), group));
        }
    }
    cues
}

/// Reads SUB at 25 frames each second.
fn parse_microdvd_sub(text: &str) -> Vec<RawCue> {
    let mut cues = Vec::new();
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
        let Some((end_frame, body)) = rest.split_once('}') else {
            continue;
        };
        let Ok(start_frame) = start_frame.parse::<u32>() else {
            continue;
        };
        let end = end_frame
            .parse::<u32>()
            .ok()
            .map(|frame| frame.saturating_mul(40));
        let body = body.replace('|', " ").trim().to_owned();
        if !body.is_empty() {
            let group = cues.len();
            cues.push(RawCue::line_of_its_own(
                start_frame.saturating_mul(40),
                end,
                body,
                group,
            ));
        }
    }
    cues
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

/// The text of a file with no time format that the parser knows, with the
/// time codes removed.
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

/// The text of the bytes. A UTF-8 BOM or a UTF-16 BOM selects the encoding.
fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, u16::from_be_bytes),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    char::decode_utf16(bytes.chunks_exact(2).map(|pair| unit([pair[0], pair[1]])))
        .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}
