//! The checks of musicindex ADR 0090 for `musicindex-tags` (section Checks).
//!
//! The `id3` crate and `lofty` cannot write a bad frame, so each test makes
//! its test file from raw bytes.

use std::path::{Path, PathBuf};

use anyhow::Result;
use id3::{Tag, TagLike, Version};
use musicindex_tags::track::{TrackTags, read_tags_checked};
use musicindex_tags::{
    BadFrameAction, BadTag, Id3v24Edit, KnownBadFrame, NoReferences, SongTags,
    read_audio_tags_checked, read_id3v2, read_podcast_tags, write_id3v24_edits, write_song_tags,
};
use tempfile::TempDir;

/// The tag flag for unsynchronisation.
const UNSYNCHRONISATION: u8 = 0x80;
/// The tag flag for an extended header.
const EXTENDED_HEADER: u8 = 0x40;

/// The MPEG audio bytes of the fixture `old-frames.mp3`, with no tag.
fn audio() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/old-frames.mp3");
    let read = read_id3v2(&path).expect("read the fixture tag");
    let bytes = std::fs::read(&path).expect("read the fixture");
    bytes[usize::try_from(read.tag_len).expect("tag length")..].to_vec()
}

fn syncsafe(value: usize) -> [u8; 4] {
    let value = u32::try_from(value).expect("size");
    assert!(value < 1 << 28, "a syncsafe size holds 28 bits");
    [
        u8::try_from((value >> 21) & 0x7F).expect("byte"),
        u8::try_from((value >> 14) & 0x7F).expect("byte"),
        u8::try_from((value >> 7) & 0x7F).expect("byte"),
        u8::try_from(value & 0x7F).expect("byte"),
    ]
}

fn plain(value: usize) -> [u8; 4] {
    u32::try_from(value).expect("size").to_be_bytes()
}

/// An ID3v2.3 frame: a plain 32-bit size.
fn frame23(id: &str, data: &[u8]) -> Vec<u8> {
    let mut frame = id.as_bytes().to_vec();
    frame.extend(plain(data.len()));
    frame.extend([0, 0]);
    frame.extend(data);
    frame
}

/// An ID3v2.4 frame: a syncsafe size.
fn frame24(id: &str, data: &[u8]) -> Vec<u8> {
    let mut frame = id.as_bytes().to_vec();
    frame.extend(syncsafe(data.len()));
    frame.extend([0, 0]);
    frame.extend(data);
    frame
}

/// An ID3v2 tag header and `body`.
fn tag(major: u8, flags: u8, body: &[u8]) -> Vec<u8> {
    let mut tag = vec![b'I', b'D', b'3', major, 0, flags];
    tag.extend(syncsafe(body.len()));
    tag.extend(body);
    tag
}

/// The content of a text frame in Latin-1.
fn latin1(text: &str) -> Vec<u8> {
    let mut data = vec![0];
    data.extend(text.as_bytes());
    data
}

/// UTF-16 with a byte order mark, little endian, with no terminator.
fn utf16(text: &str) -> Vec<u8> {
    let mut data = vec![0xFF, 0xFE];
    data.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    data
}

/// The content of a text frame in UTF-16.
fn utf16_text(text: &str) -> Vec<u8> {
    let mut data = vec![1];
    data.extend(utf16(text));
    data
}

/// A test file in `dir`: `tag`, then the audio, then an ID3v1 tag when
/// `id3v1` is true.
fn mp3(dir: &TempDir, name: &str, tag: &[u8], id3v1: bool) -> PathBuf {
    let mut bytes = tag.to_vec();
    bytes.extend(audio());
    if id3v1 {
        let mut v1 = b"TAG".to_vec();
        let mut title = b"Old ID3v1 title".to_vec();
        title.resize(30, 0);
        v1.extend(title);
        v1.resize(128, 0);
        bytes.extend(v1);
    }
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write the test file");
    path
}

/// A strict read with the `id3` crate. It fails on each bad frame.
fn strict(path: &Path) -> Tag {
    Tag::read_from_path(path).expect("a strict id3 read accepts the tag")
}

fn has_value(tags: &TrackTags, value: &str) -> bool {
    tags.tags
        .iter()
        .chain(&tags.musicindex)
        .any(|text| text.value == value)
}

fn txxx_values(tag: &Tag) -> Vec<(String, String)> {
    tag.extended_texts()
        .map(|text| (text.description.clone(), text.value.clone()))
        .collect()
}

/// R-TAG-7 items 1 to 3: a `TXXX` frame with no NUL is bad. Each read keeps
/// the good frames before and after it.
fn txxx_with_no_nul(dir: &TempDir) -> PathBuf {
    let mut body = frame23("TIT2", &latin1("Before"));
    body.extend(frame23("TXXX", &latin1("no delimiter")));
    body.extend(frame23("TPE1", &latin1("After")));
    mp3(dir, "no-nul.mp3", &tag(3, 0, &body), false)
}

#[test]
fn each_read_keeps_the_good_frames_around_a_txxx_with_no_nul() -> Result<()> {
    let dir = TempDir::new()?;
    let path = txxx_with_no_nul(&dir);

    let read = read_id3v2(&path)?;
    assert_eq!(read.version, Some(3));
    assert_eq!(read.tag.title(), Some("Before"));
    assert_eq!(read.tag.artist(), Some("After"));
    assert_eq!(read.problems.bad_frames.len(), 1);
    let bad = &read.problems.bad_frames[0];
    assert_eq!(bad.id.as_deref(), Some("TXXX"));
    assert_eq!(bad.position, 10 + 10 + 7);
    assert_eq!(bad.suite_label, None);

    let (audio, problems) = read_audio_tags_checked(&path)?;
    assert_eq!(audio.title.as_deref(), Some("Before"));
    assert_eq!(audio.artist.as_deref(), Some("After"));
    assert_eq!(problems, read.problems);

    let podcast = read_podcast_tags(&path)?;
    assert_eq!(podcast.audio.title.as_deref(), Some("Before"));
    assert_eq!(podcast.audio.artist.as_deref(), Some("After"));
    assert_eq!(podcast.problems, read.problems);

    let (display, problems) = read_tags_checked(&path)?;
    assert!(has_value(&display, "Before"), "{display:?}");
    assert!(has_value(&display, "After"), "{display:?}");
    assert!(display.duration.is_some());
    assert_eq!(problems, read.problems);
    Ok(())
}

#[test]
fn a_write_gives_a_strict_v24_tag_with_no_bad_frame() -> Result<()> {
    let dir = TempDir::new()?;
    let path = txxx_with_no_nul(&dir);
    assert!(Tag::read_from_path(&path).is_err());

    let result = write_id3v24_edits(&path, &[], &NoReferences)?;
    assert_eq!(result.bad_frames.len(), 1);

    let tag = strict(&path);
    assert_eq!(tag.version(), Version::Id3v24);
    assert_eq!(tag.title(), Some("Before"));
    assert_eq!(tag.artist(), Some("After"));
    assert!(tag.get("TXXX").is_none());
    assert!(read_id3v2(&path)?.problems.is_empty());
    Ok(())
}

/// R-TAG-7 item 4: a frame size past the end of the tag makes the frame bad.
/// The loop keeps the earlier frames and stops.
#[test]
fn a_frame_size_past_the_end_of_the_tag_stops_the_loop() -> Result<()> {
    let dir = TempDir::new()?;
    let mut body = frame24("TIT2", &latin1("Kept"));
    body.extend(b"TPE1");
    body.extend(syncsafe(1000));
    body.extend([0, 0]);
    body.extend(latin1("short"));
    let path = mp3(&dir, "past-end.mp3", &tag(4, 0, &body), false);

    let read = read_id3v2(&path)?;
    assert_eq!(read.tag.title(), Some("Kept"));
    assert_eq!(read.tag.frames().count(), 1);
    assert_eq!(read.problems.bad_frames.len(), 1);
    assert_eq!(read.problems.bad_frames[0].id.as_deref(), Some("TPE1"));

    write_id3v24_edits(&path, &[], &NoReferences)?;
    let tag = strict(&path);
    assert_eq!(tag.title(), Some("Kept"));
    assert!(tag.artist().is_none());
    Ok(())
}

/// R-TAG-7 item 4: a zero byte at the start of a frame header starts the
/// padding. The padding is not a bad frame.
#[test]
fn padding_after_the_last_frame_ends_the_loop() -> Result<()> {
    let dir = TempDir::new()?;
    let mut body = frame23("TIT2", &latin1("Padded"));
    body.extend(vec![0; 200]);
    let path = mp3(&dir, "padding.mp3", &tag(3, 0, &body), false);

    let read = read_id3v2(&path)?;
    assert_eq!(read.tag.title(), Some("Padded"));
    assert_eq!(read.tag.frames().count(), 1);
    assert!(read.problems.is_empty(), "{:?}", read.problems);
    Ok(())
}

/// R-TAG-7 item 4: an ID3v2.4 tag with plain frame sizes. The syncsafe
/// reading of the size 256 is 128, which points to no frame header, so the
/// loop uses the plain reading.
#[test]
fn a_v24_tag_with_plain_frame_sizes_keeps_each_frame() -> Result<()> {
    let dir = TempDir::new()?;
    let long_title = "T".repeat(255);
    let mut body = frame23("TIT2", &latin1(&long_title));
    body.extend(frame23("TPE1", &latin1("Plain")));
    let path = mp3(&dir, "plain-sizes.mp3", &tag(4, 0, &body), false);

    let read = read_id3v2(&path)?;
    assert_eq!(read.tag.title(), Some(long_title.as_str()));
    assert_eq!(read.tag.artist(), Some("Plain"));
    assert!(read.problems.is_empty(), "{:?}", read.problems);
    assert!(read.plain_frame_sizes);

    let (display, _) = read_tags_checked(&path)?;
    assert!(has_value(&display, "Plain"), "{display:?}");

    // The plain sizes are not correct in ID3v2.4, so a write with no edit
    // repairs the tag.
    write_id3v24_edits(&path, &[], &NoReferences)?;
    let tag = strict(&path);
    assert_eq!(tag.title(), Some(long_title.as_str()));
    assert_eq!(tag.artist(), Some("Plain"));
    assert!(!read_id3v2(&path)?.plain_frame_sizes);
    Ok(())
}

/// R-TAG-7 item 4: the loop removes the unsynchronisation of the full tag in
/// ID3v2.3 and skips the extended header by its size.
#[test]
fn a_v23_tag_with_unsynchronisation_and_an_extended_header() -> Result<()> {
    let dir = TempDir::new()?;
    let mut frames = frame23("TIT2", &[0, b'A', 0xFF, b'B']);
    frames.extend(frame23("TPE1", &latin1("Synced")));
    // The ID3v2.3 extended header: a size of 6, two flag bytes, 4 bytes of
    // padding size.
    let mut body = vec![0, 0, 0, 6, 0, 0, 0, 0, 0, 0];
    body.extend(frames);
    let mut unsynced = Vec::new();
    for byte in body {
        unsynced.push(byte);
        if byte == 0xFF {
            unsynced.push(0);
        }
    }
    let path = mp3(
        &dir,
        "unsync.mp3",
        &tag(3, UNSYNCHRONISATION | EXTENDED_HEADER, &unsynced),
        false,
    );

    let read = read_id3v2(&path)?;
    assert_eq!(read.tag.title(), Some("A\u{FF}B"));
    assert_eq!(read.tag.artist(), Some("Synced"));
    assert!(read.problems.is_empty(), "{:?}", read.problems);
    Ok(())
}

/// R-TAG-7 item 4: an encrypted frame is a bad frame.
#[test]
fn an_encrypted_frame_is_bad() -> Result<()> {
    let dir = TempDir::new()?;
    let mut body = b"TIT2".to_vec();
    let data = [0x80, b'x', b'y'];
    body.extend(syncsafe(data.len()));
    body.extend([0, 0x04]);
    body.extend(data);
    body.extend(frame24("TPE1", &latin1("Clear")));
    let path = mp3(&dir, "encrypted.mp3", &tag(4, 0, &body), false);

    let read = read_id3v2(&path)?;
    assert_eq!(read.tag.artist(), Some("Clear"));
    assert!(read.tag.title().is_none());
    assert_eq!(read.problems.bad_frames.len(), 1);
    assert_eq!(read.problems.bad_frames[0].id.as_deref(), Some("TIT2"));
    Ok(())
}

/// R-TAG-7 item 5: a write does not change a file with a bad tag header.
#[test]
fn a_write_does_not_change_a_file_with_a_bad_tag_header() -> Result<()> {
    let dir = TempDir::new()?;
    let mut header = vec![b'I', b'D', b'3', 4, 0, 0, 0x80, 0, 0, 0];
    header.extend(frame24("TIT2", &latin1("Lost")));
    let path = mp3(&dir, "bad-header.mp3", &header, false);
    let before = std::fs::read(&path)?;

    let read = read_id3v2(&path)?;
    assert!(read.problems.bad_tag.is_some());
    assert_eq!(read.tag.frames().count(), 0);

    let error = write_id3v24_edits(&path, &[], &NoReferences).expect_err("a bad tag header");
    assert!(error.downcast_ref::<BadTag>().is_some(), "{error:#}");
    assert_eq!(std::fs::read(&path)?, before);

    // The display read gives no ID3v2 frame.
    let (display, problems) = read_tags_checked(&path)?;
    assert!(problems.bad_tag.is_some());
    assert!(!has_value(&display, "Lost"), "{display:?}");
    Ok(())
}

/// R-TAG-8 item 2: `TYER`, `TDAT` and `TIME` merge into one `TDRC`.
#[test]
fn tyer_tdat_and_time_merge_into_one_tdrc() -> Result<()> {
    let dir = TempDir::new()?;
    let mut body = frame23("TYER", &latin1("2001"));
    body.extend(frame23("TDAT", &latin1("2412")));
    body.extend(frame23("TIME", &latin1("1305")));
    let path = mp3(&dir, "date.mp3", &tag(3, 0, &body), false);

    let result = write_id3v24_edits(&path, &[], &NoReferences)?;
    assert!(result.removed_frames.is_empty(), "{result:?}");

    let tag = strict(&path);
    assert_eq!(tag.version(), Version::Id3v24);
    let tdrc = tag
        .frames()
        .filter(|frame| frame.id() == "TDRC")
        .collect::<Vec<_>>();
    assert_eq!(tdrc.len(), 1);
    assert_eq!(tdrc[0].content().text(), Some("2001-12-24T13:05"));
    for old in ["TYER", "TDAT", "TIME"] {
        assert!(tag.get(old).is_none(), "{old}");
    }
    Ok(())
}

/// R-TAG-8 items 4 and 5: a write with no edit removes an ID3v1 tag.
#[test]
fn a_write_with_no_edit_removes_id3v1() -> Result<()> {
    let dir = TempDir::new()?;
    let body = frame24("TIT2", &latin1("Current"));
    let path = mp3(&dir, "id3v1.mp3", &tag(4, 0, &body), true);
    assert!(read_id3v2(&path)?.has_id3v1);

    let (display, _) = read_tags_checked(&path)?;
    assert!(!has_value(&display, "Old ID3v1 title"), "{display:?}");

    let result = write_id3v24_edits(&path, &[], &NoReferences)?;
    assert!(result.removed_id3v1);
    assert!(!read_id3v2(&path)?.has_id3v1);
    assert_eq!(after_tag(&path), audio());
    assert_eq!(strict(&path).title(), Some("Current"));
    Ok(())
}

/// R-TAG-9 items 2 and 3: a bad suite frame is removed and written again from
/// the data of the app.
#[test]
fn a_bad_suite_frame_gets_the_frame_of_the_app_data() -> Result<()> {
    let dir = TempDir::new()?;
    let mut data = vec![1];
    data.extend(utf16("Podcast Feed Guid"));
    data.extend([0, 0]);
    // A lone surrogate: the value is not UTF-16.
    data.extend([0xFF, 0xFE, 0x00, 0xD8]);
    let mut body = frame24("TXXX", &data);
    body.extend(frame24("TIT2", &latin1("Song")));
    let path = mp3(&dir, "bad-suite.mp3", &tag(4, 0, &body), false);

    let read = read_id3v2(&path)?;
    assert_eq!(read.problems.bad_frames.len(), 1);
    assert_eq!(
        read.problems.bad_frames[0].suite_label.as_deref(),
        Some("TXXX:Podcast Feed Guid")
    );

    let song = SongTags {
        feed_guid: "feed-guid".into(),
        item_guid: "item-guid".into(),
    };
    write_song_tags(&path, &song, &[], &NoReferences)?;
    let tag = strict(&path);
    let feed = txxx_values(&tag)
        .into_iter()
        .filter(|(description, _)| description == "Podcast Feed Guid")
        .collect::<Vec<_>>();
    assert_eq!(feed, [("Podcast Feed Guid".into(), "feed-guid".into())]);
    assert_eq!(tag.title(), Some("Song"));
    Ok(())
}

/// R-TAG-9 item 2: a bad `TXXX` frame with an unreadable description is not a
/// suite frame. The write removes it.
#[test]
fn a_bad_txxx_with_an_unreadable_description_is_removed() -> Result<()> {
    let dir = TempDir::new()?;
    // UTF-8 with bytes that are not UTF-8, then a NUL and the value.
    let mut data = vec![3, 0xC3, 0x28, 0xFF, 0];
    data.extend(b"value");
    let mut body = frame24("TXXX", &data);
    body.extend(frame24("TIT2", &latin1("Song")));
    let path = mp3(&dir, "bad-description.mp3", &tag(4, 0, &body), false);

    let read = read_id3v2(&path)?;
    assert_eq!(read.problems.bad_frames.len(), 1);
    assert_eq!(read.problems.bad_frames[0].suite_label, None);

    write_id3v24_edits(&path, &[], &NoReferences)?;
    let tag = strict(&path);
    assert!(tag.get("TXXX").is_none());
    assert_eq!(tag.title(), Some("Song"));
    Ok(())
}

/// The six good frames of the Strange Love tag (ADR 0090 §Context).
const STRANGE_LOVE_FRAMES: [(&str, &str); 5] = [
    ("TALB", "Strange Love"),
    ("TIT2", "Dirt and Sand"),
    ("TPE1", "Strange Love"),
    ("TRCK", "1"),
    ("TLEN", "215000"),
];
const STRANGE_LOVE_COMMENT: &str = "Transcoded with Nero D2D";

/// A copy of the Strange Love tag: ID3v2.3, six UTF-16 frames, then two
/// `PRIV` frames with a size of 0 and padding.
fn strange_love_tag() -> Vec<u8> {
    let mut body = Vec::new();
    for (id, text) in STRANGE_LOVE_FRAMES {
        body.extend(frame23(id, &utf16_text(text)));
    }
    let mut comment = vec![1];
    comment.extend(b"eng");
    comment.extend(utf16(""));
    comment.extend([0, 0]);
    comment.extend(utf16(STRANGE_LOVE_COMMENT));
    body.extend(frame23("COMM", &comment));
    body.extend(frame23("PRIV", &[]));
    body.extend(frame23("PRIV", &[]));
    body.resize(1014 - 10, 0);
    tag(3, 0, &body)
}

/// The test file of each entry of the list of known bad frames (R-TAG-9
/// item 4), with the ID of the bad frame and its text (`None` when the frame
/// has no text). A new entry does not compile until it has a test file here.
fn known_entry_file(
    dir: &TempDir,
    entry: KnownBadFrame,
) -> (PathBuf, &'static str, Option<&'static str>) {
    match entry {
        KnownBadFrame::ZeroSize => (
            mp3(dir, "strange-love.mp3", &strange_love_tag(), false),
            "PRIV",
            None,
        ),
        KnownBadFrame::LengthNotAWholeNumber => (bad_length_file(dir), "TLEN", Some("abc")),
    }
}

#[test]
fn each_known_bad_frame_has_a_test_file() -> Result<()> {
    for entry in KnownBadFrame::ALL {
        let dir = TempDir::new()?;
        let (path, id, bad_text) = known_entry_file(&dir, entry);
        let read = read_id3v2(&path)?;
        assert!(
            read.problems
                .bad_frames
                .iter()
                .any(|bad| bad.known == Some(entry) && bad.id.as_deref() == Some(id)),
            "{entry:?}: {:?}",
            read.problems
        );
        write_id3v24_edits(&path, &[], &NoReferences)?;
        // A removed frame can come back with new data, for example the
        // measured TLEN (musicindex ADR 0091 R-TAG-11 item 4). The bad
        // content does not come back.
        let tag = strict(&path);
        let frame = tag.get(id);
        match entry.action() {
            BadFrameAction::Remove => assert!(
                frame.is_none_or(|frame| frame.content().text() != bad_text),
                "{entry:?}"
            ),
            BadFrameAction::Repair => assert!(frame.is_some(), "{entry:?}"),
        }
    }
    Ok(())
}

/// The Strange Love tag: after the write, the six frames are in an ID3v2.4
/// tag and the `PRIV` frames are gone.
#[test]
fn the_strange_love_tag_keeps_six_frames_and_loses_the_priv_frames() -> Result<()> {
    let dir = TempDir::new()?;
    let path = mp3(&dir, "strange-love.mp3", &strange_love_tag(), false);
    assert!(Tag::read_from_path(&path).is_err());

    let read = read_id3v2(&path)?;
    assert_eq!(read.tag.frames().count(), 6);
    assert_eq!(read.problems.bad_frames.len(), 2);
    assert!(
        read.problems
            .bad_frames
            .iter()
            .all(|bad| bad.known == Some(KnownBadFrame::ZeroSize))
    );

    let result = write_id3v24_edits(&path, &[], &NoReferences)?;
    assert_eq!(result.bad_frames.len(), 2);

    let tag = strict(&path);
    assert_eq!(tag.version(), Version::Id3v24);
    assert_eq!(tag.frames().count(), 6);
    for (id, text) in STRANGE_LOVE_FRAMES {
        assert_eq!(
            tag.get(id).and_then(|frame| frame.content().text()),
            Some(text),
            "{id}"
        );
    }
    let comments = tag.comments().collect::<Vec<_>>();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, STRANGE_LOVE_COMMENT);
    assert!(tag.get("PRIV").is_none());
    Ok(())
}

/// The bytes after the ID3v2 tag of `path`, from the frame loop.
fn after_tag(path: &Path) -> Vec<u8> {
    let read = read_id3v2(path).expect("read the tag");
    let bytes = std::fs::read(path).expect("read the file");
    bytes[usize::try_from(read.tag_len).expect("tag length")..].to_vec()
}

/// A write keeps the audio bytes equal when the old tag has an extended
/// header or a footer. The `id3` crate does not count them in the length of
/// the old tag, so the writer uses the length of the frame loop.
#[test]
fn a_write_keeps_the_audio_after_an_extended_header_or_a_footer() -> Result<()> {
    let dir = TempDir::new()?;
    let mut v23 = vec![0, 0, 0, 6, 0, 0, 0, 0, 0, 0];
    v23.extend(frame23("TIT2", &latin1("A")));
    v23.extend(frame23("TPE1", &latin1("Synced")));
    let mut v24 = vec![0, 0, 0, 6, 1, 0];
    v24.extend(frame24("TIT2", &latin1("A")));
    let mut footer = tag(4, 0x10, &frame24("TIT2", &latin1("A")));
    let size = footer[6..10].to_vec();
    footer.extend([b'3', b'D', b'I', 4, 0, 0x10]);
    footer.extend(size);

    for (name, bytes) in [
        ("extended-v23.mp3", tag(3, EXTENDED_HEADER, &v23)),
        ("extended-v24.mp3", tag(4, EXTENDED_HEADER, &v24)),
        ("footer.mp3", footer),
    ] {
        let path = mp3(&dir, name, &bytes, false);
        assert_eq!(after_tag(&path), audio(), "{name}");
        let edit = Id3v24Edit {
            frame_label: "TALB".into(),
            value: "Edit".into(),
        };
        write_id3v24_edits(&path, &[edit], &NoReferences)?;
        let tag = strict(&path);
        assert_eq!(tag.title(), Some("A"), "{name}");
        assert_eq!(tag.album(), Some("Edit"), "{name}");
        assert_eq!(after_tag(&path), audio(), "{name}");
    }
    Ok(())
}

/// R-TAG-7 item 5: an extended header with a size that is not valid makes
/// the tag header bad.
#[test]
fn an_extended_header_size_that_is_not_valid_is_a_bad_tag() -> Result<()> {
    let dir = TempDir::new()?;
    let mut body = vec![0, 0, 0, 0, 0, 0];
    body.extend(frame24("TIT2", &latin1("Lost")));
    let path = mp3(
        &dir,
        "bad-extended.mp3",
        &tag(4, EXTENDED_HEADER, &body),
        false,
    );
    assert!(read_id3v2(&path)?.problems.bad_tag.is_some());
    Ok(())
}

/// R-TAG-8 items 1 and 4: a frame of an older version in an ID3v2.4 tag is
/// a change. The display read shows the ID3v2.4 equivalent.
#[test]
fn an_old_frame_in_a_v24_tag_is_a_change() -> Result<()> {
    let dir = TempDir::new()?;
    let mut body = frame24("TIT2", &latin1("x"));
    body.extend(frame24("TYER", &latin1("2001")));
    body.extend(frame24("TORY", &latin1("1999")));
    let path = mp3(&dir, "old-in-v24.mp3", &tag(4, 0, &body), false);

    let (display, _) = read_tags_checked(&path)?;
    assert!(has_value(&display, "2001"), "{display:?}");
    assert!(
        display.tags.iter().all(|text| text.key != "TYER"),
        "{display:?}"
    );

    write_id3v24_edits(&path, &[], &NoReferences)?;
    let tag = strict(&path);
    assert!(tag.get("TYER").is_none());
    assert!(tag.get("TORY").is_none());
    assert_eq!(
        tag.get("TDRC").and_then(|frame| frame.content().text()),
        Some("2001")
    );
    assert_eq!(
        tag.get("TDOR").and_then(|frame| frame.content().text()),
        Some("1999")
    );
    Ok(())
}

/// R-TAG-8 item 1: a `TYER` edit goes into `TDRC`.
#[test]
fn a_tyer_edit_writes_tdrc() -> Result<()> {
    let dir = TempDir::new()?;
    let path = mp3(
        &dir,
        "tyer-edit.mp3",
        &tag(4, 0, &frame24("TIT2", &latin1("x"))),
        false,
    );
    let edit = Id3v24Edit {
        frame_label: "TYER".into(),
        value: "2001".into(),
    };
    write_id3v24_edits(&path, &[edit], &NoReferences)?;
    let tag = strict(&path);
    assert!(tag.get("TYER").is_none());
    assert_eq!(
        tag.get("TDRC").and_then(|frame| frame.content().text()),
        Some("2001")
    );
    Ok(())
}

/// R-TAG-7 item 5 and R-TAG-8 item 5: with a bad tag header, the display read
/// shows no ID3v1 data.
#[test]
fn the_display_read_of_a_bad_tag_header_shows_no_id3v1_data() -> Result<()> {
    let dir = TempDir::new()?;
    let mut header = vec![b'I', b'D', b'3', 3, 0, 0, 0x80, 0, 0, 0x11];
    header.extend(frame23("TIT2", &latin1("Lost")));
    let path = mp3(&dir, "bad-header-v1.mp3", &header, true);
    let (display, problems) = read_tags_checked(&path)?;
    assert!(problems.bad_tag.is_some());
    assert!(!has_value(&display, "Old ID3v1 title"), "{display:?}");
    Ok(())
}

/// Orchestrator choice of 2026-11-22 under R-TAG-9 item 1: a "/" in an
/// ID3v2.2 or ID3v2.3 artist is not a sure separator. The reads and the
/// write keep one value with the "/".
#[test]
fn a_slash_in_a_v23_artist_stays_one_value() -> Result<()> {
    let dir = TempDir::new()?;
    let mut body = frame23("TPE1", &latin1("AC/DC"));
    body.extend(frame23("TCOM", &utf16_text("A/B")));
    let path = mp3(&dir, "slash.mp3", &tag(3, 0, &body), false);

    let read = read_id3v2(&path)?;
    assert_eq!(read.tag.artist(), Some("AC/DC"));
    let (display, _) = read_tags_checked(&path)?;
    assert!(has_value(&display, "AC/DC"), "{display:?}");
    assert!(!has_value(&display, "AC"), "{display:?}");
    assert!(!has_value(&display, "DC"), "{display:?}");

    write_id3v24_edits(&path, &[], &NoReferences)?;
    let tag = strict(&path);
    assert_eq!(tag.version(), Version::Id3v24);
    let artists = tag
        .get("TPE1")
        .and_then(|frame| frame.content().text_values());
    assert_eq!(
        artists.map(Iterator::collect::<Vec<_>>),
        Some(vec!["AC/DC"])
    );
    let composers = tag
        .get("TCOM")
        .and_then(|frame| frame.content().text_values());
    assert_eq!(
        composers.map(Iterator::collect::<Vec<_>>),
        Some(vec!["A/B"])
    );
    Ok(())
}

/// The same choice for an ID3v2.2 artist.
#[test]
fn a_slash_in_a_v22_artist_stays_one_value() -> Result<()> {
    let dir = TempDir::new()?;
    let data = latin1("AC/DC");
    let mut body = b"TP1".to_vec();
    body.extend(&plain(data.len())[1..]);
    body.extend(data);
    let path = mp3(&dir, "slash-v22.mp3", &tag(2, 0, &body), false);
    assert_eq!(read_id3v2(&path)?.tag.artist(), Some("AC/DC"));
    Ok(())
}

/// A file with a `TLEN` of "abc" (musicindex ADR 0091 R-TAG-11 item 4).
fn bad_length_file(dir: &TempDir) -> PathBuf {
    let mut body = frame24("TIT2", &latin1("Song"));
    body.extend(frame24("TLEN", &latin1("abc")));
    mp3(dir, "bad-length.mp3", &tag(4, 0, &body), false)
}

/// The `TLEN` text of `path` after a write, from a strict read.
fn length_text(path: &Path) -> Option<String> {
    strict(path)
        .get("TLEN")
        .and_then(|frame| frame.content().text())
        .map(str::to_owned)
}

/// The length in whole milliseconds that the display read measures.
fn measured_ms(path: &Path) -> String {
    let (display, _) = read_tags_checked(path).expect("read the display tags");
    display
        .duration
        .expect("the test audio has a length")
        .as_millis()
        .to_string()
}

fn album_edit() -> Id3v24Edit {
    Id3v24Edit {
        frame_label: "TALB".into(),
        value: "Edit".into(),
    }
}

#[test]
fn a_write_keeps_the_tlen_of_the_file() -> Result<()> {
    let dir = TempDir::new()?;
    let mut body = frame24("TIT2", &latin1("Song"));
    body.extend(frame24("TLEN", &latin1("209484")));
    let path = mp3(&dir, "length.mp3", &tag(4, 0, &body), false);
    let edit = Id3v24Edit {
        frame_label: "TLEN".into(),
        value: "208000".into(),
    };
    let result = write_id3v24_edits(&path, &[edit, album_edit()], &NoReferences)?;
    assert_eq!(
        length_text(&path).as_deref(),
        Some("209484"),
        "musicindex ADR 0091 R-TAG-11 item 1: a write keeps the TLEN of the file"
    );
    assert!(
        result
            .skipped
            .iter()
            .any(|reason| reason.contains("R-TAG-11")),
        "musicindex ADR 0091 R-TAG-11 item 1: the write reports the ignored TLEN edit: {:?}",
        result.skipped
    );
    Ok(())
}

#[test]
fn a_write_gives_the_measured_length_when_the_file_has_no_tlen() -> Result<()> {
    let dir = TempDir::new()?;
    let path = mp3(
        &dir,
        "no-length.mp3",
        &tag(4, 0, &frame24("TIT2", &latin1("Song"))),
        false,
    );
    let measured = measured_ms(&path);
    let edit = Id3v24Edit {
        frame_label: "TLEN".into(),
        value: "208000".into(),
    };
    write_id3v24_edits(&path, &[edit, album_edit()], &NoReferences)?;
    assert_eq!(
        length_text(&path),
        Some(measured),
        "musicindex ADR 0091 R-TAG-11 item 2: a write gives the measured length, not the edit"
    );
    Ok(())
}

#[test]
fn a_write_gives_no_tlen_when_the_length_cannot_be_measured() -> Result<()> {
    let dir = TempDir::new()?;
    let mut bytes = tag(4, 0, &frame24("TIT2", &latin1("Song")));
    bytes.extend(vec![0x55; 4096]);
    let path = dir.path().join("no-audio.mp3");
    std::fs::write(&path, bytes)?;
    write_id3v24_edits(&path, &[album_edit()], &NoReferences)?;
    assert_eq!(strict(&path).album(), Some("Edit"));
    assert_eq!(
        length_text(&path),
        None,
        "musicindex ADR 0091 R-TAG-11 item 3: a write gives no TLEN when it cannot measure a length"
    );
    Ok(())
}

#[test]
fn a_tlen_that_is_not_a_whole_number_gets_the_measured_length() -> Result<()> {
    let dir = TempDir::new()?;
    let path = bad_length_file(&dir);
    let read = read_id3v2(&path)?;
    assert!(
        read.problems
            .bad_frames
            .iter()
            .any(|bad| bad.known == Some(KnownBadFrame::LengthNotAWholeNumber)),
        "musicindex ADR 0091 R-TAG-11 item 4: a TLEN of abc is a bad frame: {:?}",
        read.problems
    );
    let measured = measured_ms(&path);
    write_id3v24_edits(&path, &[], &NoReferences)?;
    assert_eq!(
        length_text(&path),
        Some(measured),
        "musicindex ADR 0091 R-TAG-11 item 4: a TLEN of abc gets the measured length"
    );
    Ok(())
}
