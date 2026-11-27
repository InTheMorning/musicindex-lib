//! The checks of musicindex ADR 0103 §Checks for the parser of R-LYRIC-3, and
//! the line numbers of musicindex ADR 0104 R-LYRIC-2 item 3.

use musicindex_transcript::{Cue, ParseError, Transcript, parse, parse_bytes};
use serde_json::{Value, json};

/// The example of the podcast namespace, `docs/examples/transcripts/example.json`
/// (CC0, musicindex-research `sources/podcast-namespace`).
const NAMESPACE_EXAMPLE: &str = include_str!("fixtures/namespace-example.json");

fn cue(start_ms: u32, end_ms: Option<u32>, body: &str, line: u32) -> Cue {
    Cue {
        start_ms,
        end_ms,
        body: body.to_owned(),
        speaker: None,
        line,
    }
}

/// Each segment of one line has the same `line`, and `line` does not
/// decrease (musicindex ADR 0104 R-LYRIC-2 item 3).
fn assert_lines_do_not_decrease(transcript: &Transcript) {
    for pair in transcript.cues.windows(2) {
        assert!(pair[0].line <= pair[1].line, "{transcript:?}");
    }
    if let Some(first) = transcript.cues.first() {
        assert_eq!(first.line, 0);
    }
}

/// R-LYRIC-3 item 2: the parser removes a BOM.
#[test]
fn an_srt_file_with_a_bom_gives_cues_with_no_bom() {
    let text = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,000\r\nHello\r\n\r\n2\r\n00:00:02,500 --> 00:00:03,000\r\nworld\r\n";
    let transcript = parse(text, None).expect("parse");
    assert_eq!(
        transcript.cues,
        vec![
            cue(1000, Some(2000), "Hello", 0),
            cue(2500, Some(3000), "world", 1)
        ]
    );
    assert_eq!(transcript.plain_text, "Hello\nworld");
}

/// R-LYRIC-3 item 2: a UTF-8 BOM and a UTF-16 BOM in the bytes.
#[test]
fn bytes_with_a_bom_give_the_same_cues() {
    let text = "1\n00:00:01,000 --> 00:00:02,000\nHello\n";
    let expected = parse(text, None).expect("parse");

    let mut utf8 = vec![0xEF, 0xBB, 0xBF];
    utf8.extend_from_slice(text.as_bytes());
    assert_eq!(parse_bytes(&utf8, None), Ok(expected.clone()));

    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(parse_bytes(&utf16, None), Ok(expected.clone()));

    let mut utf16_be = vec![0xFE, 0xFF];
    utf16_be.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    assert_eq!(parse_bytes(&utf16_be, None), Ok(expected));
}

/// R-LYRIC-3 item 4: the types `application/srt` and `application/x-subrip`
/// are SRT.
#[test]
fn the_subrip_types_select_subrip() {
    let text = "1\n00:00:01,250 --> 00:00:03,000\nHello world\n";
    for mime_type in [
        "application/srt",
        "application/x-subrip",
        "Application/X-SubRip; charset=utf-8",
    ] {
        let transcript = parse(text, Some(mime_type)).expect(mime_type);
        assert_eq!(
            transcript.cues,
            vec![cue(1250, Some(3000), "Hello world", 0)],
            "{mime_type}"
        );
    }
}

/// R-LYRIC-3 item 3: the cues are in sequence by start time, and the first of
/// two cues that overlap stops at the start of the second.
#[test]
fn cues_that_overlap_are_sorted_and_cut() {
    let text = "2\n00:00:04,000 --> 00:00:06,000\nsecond\n\n\
                1\n00:00:01,000 --> 00:00:05,000\nfirst\n";
    let transcript = parse(text, Some("application/srt")).expect("parse");
    assert_eq!(
        transcript.cues,
        vec![
            cue(1000, Some(4000), "first", 0),
            cue(4000, Some(6000), "second", 1)
        ]
    );
    assert_eq!(transcript.plain_text, "first\nsecond");
}

/// An LRC cue has no end time in the file. It ends at the start of the
/// subsequent cue, and the last cue has no end time. A row with two times
/// gives two cues in sequence.
#[test]
fn lrc_cues_end_at_the_subsequent_cue() {
    let text = "[ar:Someone]\n[00:01.00][00:05.00]chorus\n[00:03.50]verse\n";
    let transcript = parse(text, None).expect("parse");
    assert_eq!(
        transcript.cues,
        vec![
            cue(1000, Some(3500), "chorus", 0),
            cue(3500, Some(5000), "verse", 1),
            cue(5000, None, "chorus", 2)
        ]
    );
}

/// R-LYRIC-3 item 6: the formats of the private parser stay.
#[test]
fn vtt_sub_and_plain_text_stay() {
    let vtt = parse(
        "WEBVTT\n\n00:00:00.500 --> 00:00:01.000\nHi\n",
        Some("text/vtt"),
    )
    .expect("vtt");
    assert_eq!(vtt.cues, vec![cue(500, Some(1000), "Hi", 0)]);

    let sub = parse("{25}{50}One|two\n", None).expect("sub");
    assert_eq!(sub.cues, vec![cue(1000, Some(2000), "One two", 0)]);

    let plain = parse("First row\n\nSecond row\n", Some("text/plain")).expect("plain");
    assert!(plain.cues.is_empty());
    assert_eq!(plain.plain_text, "First row\nSecond row");

    assert_eq!(parse("\u{feff}\n \n", None), Err(ParseError::Empty));
}

/// R-LYRIC-3 item 5: the namespace example gives its segments.
#[test]
fn the_namespace_example_gives_its_segments() {
    let transcript = parse(NAMESPACE_EXAMPLE, Some("application/json")).expect("parse");
    let expected = [
        (500, 750, "I", "Darth Vader"),
        (1000, 1250, "am", "Darth Vader"),
        (1500, 2000, "your", "Darth Vader"),
        (2250, 2500, "father.", "Darth Vader"),
        (2750, 3000, "Nooooo", "Luke"),
    ];
    assert_eq!(transcript.cues.len(), expected.len());
    for (cue, (start, end, body, speaker)) in transcript.cues.iter().zip(expected) {
        assert_eq!(cue.start_ms, start);
        assert_eq!(cue.end_ms, Some(end));
        assert_eq!(cue.body, body);
        assert_eq!(cue.speaker.as_deref(), Some(speaker));
    }
    assert_lines_do_not_decrease(&transcript);

    let output = serde_json::to_value(transcript.to_namespace(None)).expect("serialize");
    assert_eq!(output["version"], "1.0.0");
    let source: Value = serde_json::from_str(NAMESPACE_EXAMPLE).expect("example");
    let output_segments = output["segments"].as_array().expect("segments");
    let source_segments = source["segments"].as_array().expect("segments");
    for (out, src) in output_segments.iter().zip(source_segments) {
        for field in ["speaker", "body"] {
            assert_eq!(out[field], src[field], "{field}");
        }
        for field in ["startTime", "endTime"] {
            assert_eq!(out[field].as_f64(), src[field].as_f64(), "{field}");
        }
    }
    let lines = transcript
        .cues
        .iter()
        .map(|cue| cue.line)
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        [0, 0, 0, 0, 1],
        "the words of one speaker are one line"
    );
    assert_eq!(transcript.plain_text, "I am your father.\nNooooo");
}

/// musicindex ADR 0103 R-LYRIC-2 item 1: each segment has `endTime`. The last
/// LRC cue ends at the end of the audio when the caller knows it, and else at
/// its start.
#[test]
fn each_segment_has_an_end_time() {
    let transcript = parse("[00:01.00]one\n[00:02.00]two\n", None).expect("parse");
    let ends = |audio_end_ms| {
        transcript
            .to_namespace(audio_end_ms)
            .segments
            .iter()
            .map(|segment| segment.end_time)
            .collect::<Vec<_>>()
    };
    assert_eq!(ends(None), [2.0, 2.0]);
    assert_eq!(ends(Some(9_000)), [2.0, 9.0]);
    assert_eq!(ends(Some(1_500)), [2.0, 2.0]);
}

/// musicindex ADR 0104 R-LYRIC-2 item 3: a body that ends with a line break
/// ends its line, as in the example of the namespace document.
#[test]
fn a_line_break_in_a_body_ends_the_line() {
    let text = json!({
        "version": "1.0.0",
        "segments": [
            { "speaker": "A", "startTime": 0.5, "endTime": 0.75, "body": "I" },
            { "speaker": "A", "startTime": 1, "endTime": 1.25, "body": "am" },
            { "speaker": "A", "startTime": 2.25, "endTime": 2.5, "body": "father.\n" },
            { "speaker": "A", "startTime": 2.6, "endTime": 2.7, "body": "Yes" },
            { "speaker": "B", "startTime": 2.75, "endTime": 3.0, "body": "Nooooo" }
        ]
    })
    .to_string();
    let transcript = parse(&text, None).expect("parse");
    let lines = transcript
        .cues
        .iter()
        .map(|cue| cue.line)
        .collect::<Vec<_>>();
    assert_eq!(lines, [0, 0, 0, 1, 2]);
    assert_eq!(transcript.plain_text, "I am father.\nYes\nNooooo");
    assert_lines_do_not_decrease(&transcript);
}

/// musicindex ADR 0104 R-LYRIC-2 item 3: the field `line` of the file stays
/// when it does not decrease, and the output example of the ADR comes back.
#[test]
fn the_line_field_of_the_file_stays() {
    let example = json!({
        "version": "1.0.0",
        "segments": [
            { "startTime": 12.0, "endTime": 12.4, "body": "Hold", "line": 0 },
            { "startTime": 12.4, "endTime": 13.1, "body": "on", "line": 0 },
            { "startTime": 14.0, "endTime": 14.6, "body": "now", "line": 1 }
        ]
    });
    let transcript = parse(&example.to_string(), None).expect("parse");
    assert_eq!(
        serde_json::to_value(transcript.to_namespace(None)).expect("serialize"),
        example
    );
    assert_eq!(transcript.plain_text, "Hold on\nnow");
}

/// A `line` field that decreases is not used, and each body of more than one
/// word is a line.
/// After the sort, `line` does not decrease.
#[test]
fn segments_out_of_sequence_get_lines_that_do_not_decrease() {
    let text = json!({
        "segments": [
            { "startTime": 3.0, "body": "c c", "line": 1 },
            { "startTime": 1.0, "body": "a a", "line": 0 },
            { "startTime": 2.0, "body": "b b", "line": 0 },
            { "startTime": "bad", "body": "ignored" },
            { "startTime": 4.0, "body": "  ", "line": 1 }
        ]
    })
    .to_string();
    let transcript = parse(&text, None).expect("parse");
    assert_eq!(
        transcript.cues,
        vec![
            cue(1000, Some(2000), "a a", 0),
            cue(2000, Some(3000), "b b", 1),
            cue(3000, None, "c c", 2)
        ]
    );
    assert_lines_do_not_decrease(&transcript);
}
