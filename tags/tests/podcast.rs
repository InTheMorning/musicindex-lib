//! The checks of musicindex ADR 0101 for `musicindex-tags` (section Checks).

use std::path::{Path, PathBuf};

use anyhow::Result;
use id3::frame::{Comment, Lyrics, SynchronisedLyrics, SynchronisedLyricsType, TimestampFormat};
use id3::{Content, Frame, Tag, TagLike, Version};
use musicindex_tags::{
    ChapterEntry, EpisodeTags, Id3v24Edit, NoReferences, PodcastTags, SongTags, Split, SplitTarget,
    TooManyChapters, frames, read_podcast_tags, write_episode_tags, write_id3v24_edits,
    write_song_tags,
};
use serde_json::json;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A copy of the fixture `name` in `dir`.
fn copy_fixture(dir: &TempDir, name: &str) -> PathBuf {
    let copy = dir.path().join(name);
    std::fs::copy(fixture(name), &copy).expect("copy fixture");
    copy
}

/// A copy of `old-frames.mp3` that also holds an old `COMM`, `USLT` and
/// `SYLT` frame. The fixture holds the old `TXXX` frames.
fn mp3_with_each_old_frame(dir: &TempDir) -> PathBuf {
    let path = copy_fixture(dir, "old-frames.mp3");
    let mut tag = Tag::read_from_path(&path).expect("read fixture tag");
    tag.add_frame(Frame::with_content(
        "COMM",
        Content::Comment(Comment {
            lang: "eng".into(),
            description: "MusicIndex Album Description".into(),
            text: "old feed description".into(),
        }),
    ));
    tag.add_frame(Frame::with_content(
        "USLT",
        Content::Lyrics(Lyrics {
            lang: "eng".into(),
            description: "MusicIndex Transcript".into(),
            text: "old transcript".into(),
        }),
    ));
    tag.add_frame(Frame::with_content(
        "SYLT",
        Content::SynchronisedLyrics(SynchronisedLyrics {
            lang: "eng".into(),
            timestamp_format: TimestampFormat::Ms,
            content_type: SynchronisedLyricsType::Transcription,
            description: "MusicIndex Transcript".into(),
            content: vec![(0, "old transcript".into())],
        }),
    ));
    tag.write_to_path(&path, Version::Id3v24)
        .expect("write old frames");
    path
}

fn old_id3_labels(path: &Path) -> Vec<String> {
    let tag = Tag::read_from_path(path).expect("read tag");
    tag.frames()
        .filter_map(|frame| {
            let description = match frame.content() {
                Content::ExtendedText(text) => &text.description,
                Content::Comment(comment) => &comment.description,
                Content::Lyrics(lyrics) => &lyrics.description,
                Content::SynchronisedLyrics(lyrics) => &lyrics.description,
                _ => return None,
            };
            frames::is_old_frame(frame.id(), description)
                .then(|| format!("{}:{description}", frame.id()))
        })
        .collect()
}

fn song() -> SongTags {
    SongTags {
        feed_guid: "song-feed-guid".into(),
        item_guid: "song-item-guid".into(),
        image: None,
    }
}

fn split(start_time: f64, duration: f64, title: Option<&str>, target: SplitTarget) -> Split {
    Split {
        start_time,
        duration,
        remote_percentage: None,
        title: title.map(str::to_owned),
        artist: title.map(|_| "Song Artist".to_owned()),
        author: None,
        target,
    }
}

fn remote(
    feed_guid: &str,
    item_guid: Option<&str>,
    value_routes: Option<serde_json::Value>,
) -> SplitTarget {
    SplitTarget::Remote {
        feed_guid: feed_guid.into(),
        item_guid: item_guid.map(str::to_owned),
        value_routes,
        image_url: None,
    }
}

/// The image URL of the song `item-a`.
const SONG_IMAGE: &str = "https://example.com/song-a.jpg";

/// The value block of the song `item-a`, in the value form of
/// `TXXX:Podcast Value Routes`.
fn song_routes() -> serde_json::Value {
    json!([{"recipient_name": "Song Artist", "route_type": "node", "split": 100}])
}

/// Three splits out of time order: a remote split with both GUIDs and the
/// routes of its song, a local split and a remote split with no item GUID and
/// so no routes.
fn episode() -> EpisodeTags {
    let mut first = split(
        10.0,
        180.5,
        Some("First Song"),
        remote("feed-a", Some("item-a"), Some(song_routes())),
    );
    first.remote_percentage = Some(90);
    first.author = Some("Feed Author".into());
    if let SplitTarget::Remote { image_url, .. } = &mut first.target {
        *image_url = Some(SONG_IMAGE.into());
    }
    let local = split(
        400.0,
        60.0,
        None,
        SplitTarget::Local {
            value_routes: json!([{"address": "host@example.com", "split": 100, "type": "lnaddress"}]),
        },
    );
    let third = split(
        200.25,
        120.0,
        Some("Second Song"),
        remote("feed-b", None, None),
    );
    EpisodeTags {
        feed_guid: "show-feed-guid".into(),
        item_guid: "episode-item-guid".into(),
        value_routes: Some(r#"[{"recipient_name":"Host","split":100}]"#.into()),
        splits: vec![local, third, first],
        image: None,
    }
}

fn expected_entries() -> Vec<ChapterEntry> {
    vec![
        ChapterEntry {
            start_ms: 10_000,
            end_ms: 190_500,
            title: Some("First Song".into()),
            artist: Some("Song Artist".into()),
            author: Some("Feed Author".into()),
            feed_guid: Some("feed-a".into()),
            item_guid: Some("item-a".into()),
            remote_percentage: 90,
            value_routes: Some(song_routes()),
            image_url: Some(SONG_IMAGE.into()),
        },
        ChapterEntry {
            start_ms: 200_250,
            end_ms: 320_250,
            title: Some("Second Song".into()),
            artist: Some("Song Artist".into()),
            author: None,
            feed_guid: Some("feed-b".into()),
            item_guid: None,
            remote_percentage: 100,
            value_routes: None,
            image_url: None,
        },
        ChapterEntry {
            start_ms: 400_000,
            end_ms: 460_000,
            title: None,
            artist: None,
            author: None,
            feed_guid: None,
            item_guid: None,
            remote_percentage: 100,
            value_routes: Some(
                json!([{"address": "host@example.com", "split": 100, "type": "lnaddress"}]),
            ),
            image_url: None,
        },
    ]
}

/// R-TAG-3: a song file holds the two GUIDs of its song.
#[test]
fn song_mp3_and_flac_read_back_the_two_guids() -> Result<()> {
    let dir = TempDir::new()?;
    for name in ["old-frames.mp3", "old-frames.flac"] {
        let path = copy_fixture(&dir, name);
        write_song_tags(&path, &song(), &[], &NoReferences)?;
        let tags = read_podcast_tags(&path)?;
        assert_eq!(tags.feed_guid.as_deref(), Some("song-feed-guid"), "{name}");
        assert_eq!(tags.item_guid.as_deref(), Some("song-item-guid"), "{name}");
        assert!(tags.chapters()?.is_empty(), "{name}");
    }
    Ok(())
}

/// R-TAG-3: a GUID of the song replaces an edit with the same frame, so a
/// Vorbis comment gets one value.
#[test]
fn song_guid_replaces_an_edit_of_the_same_frame() -> Result<()> {
    let dir = TempDir::new()?;
    let path = copy_fixture(&dir, "untagged.flac");
    let edits = [Id3v24Edit {
        frame_label: frames::FEED_GUID.into(),
        value: "other".into(),
    }];
    write_song_tags(&path, &song(), &edits, &NoReferences)?;
    let tags = read_podcast_tags(&path)?;
    let values = tags
        .audio
        .fields
        .iter()
        .filter(|field| field.frame_id.eq_ignore_ascii_case(frames::FEED_GUID))
        .map(|field| field.value.as_str())
        .collect::<Vec<_>>();
    assert_eq!(values, vec!["song-feed-guid"]);
    Ok(())
}

/// R-TAG-4: an MP3 episode file reads back its GUIDs, its value block, the
/// `CHAP` subframes, the `CTOC` order and the JSON of `TXXX:Podcast
/// Chapters`.
#[test]
fn episode_mp3_reads_back_each_frame() -> Result<()> {
    let dir = TempDir::new()?;
    let path = copy_fixture(&dir, "old-frames.mp3");
    write_episode_tags(&path, &episode(), &[], &NoReferences)?;
    let tags = read_podcast_tags(&path)?;

    assert_eq!(tags.feed_guid.as_deref(), Some("show-feed-guid"));
    assert_eq!(tags.item_guid.as_deref(), Some("episode-item-guid"));
    assert_eq!(
        tags.value_routes.as_deref(),
        Some(r#"[{"recipient_name":"Host","split":100}]"#)
    );
    assert_eq!(tags.toc, vec!["chp0", "chp1", "chp2"]);

    let chapters = &tags.chapter_frames;
    assert_eq!(chapters.len(), 3);
    let ids = chapters
        .iter()
        .map(|chapter| chapter.element_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["chp0", "chp1", "chp2"]);
    let times = chapters
        .iter()
        .map(|chapter| (chapter.start_ms, chapter.end_ms))
        .collect::<Vec<_>>();
    assert_eq!(
        times,
        vec![(10_000, 190_500), (200_250, 320_250), (400_000, 460_000)]
    );

    assert_eq!(chapters[0].title.as_deref(), Some("First Song"));
    assert_eq!(chapters[0].feed_guid.as_deref(), Some("feed-a"));
    assert_eq!(chapters[0].item_guid.as_deref(), Some("item-a"));
    // R-TAG-4 item 5: no item GUID, so no item GUID subframe.
    assert_eq!(chapters[1].feed_guid.as_deref(), Some("feed-b"));
    assert_eq!(chapters[1].item_guid, None);
    // R-TAG-4 item 2: no title gives no TIT2. A local split has no GUID.
    assert_eq!(chapters[2].title, None);
    assert_eq!(chapters[2].feed_guid, None);
    assert_eq!(chapters[2].item_guid, None);

    assert_eq!(tags.chapters()?, expected_entries());
    Ok(())
}

/// R-TAG-4 item 4: `author` is optional, and `artist` stays.
#[test]
fn episode_chapter_json_has_author_only_when_given() -> Result<()> {
    let dir = TempDir::new()?;
    let path = copy_fixture(&dir, "old-frames.mp3");
    write_episode_tags(&path, &episode(), &[], &NoReferences)?;
    let json = read_podcast_tags(&path)?
        .chapters_json
        .expect("chapters frame");
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let entries = value.as_array().expect("array");
    assert_eq!(entries[0]["author"], "Feed Author");
    assert_eq!(entries[0]["artist"], "Song Artist");
    assert!(entries[1].get("author").is_none());
    assert!(entries[1]["artist"].is_string());
    assert!(entries[2]["artist"].is_null());
    Ok(())
}

/// Musicindex ADR 0101 R-TAG-6: a remote entry holds the routes of its song,
/// or JSON `null` when the writer has no routes. A local entry holds its own
/// recipients. The reader gives back the same entries in MP3 and FLAC.
#[test]
fn remote_entry_holds_the_routes_of_its_song_or_null() -> Result<()> {
    let dir = TempDir::new()?;
    for name in ["old-frames.mp3", "old-frames.flac"] {
        let path = copy_fixture(&dir, name);
        write_episode_tags(&path, &episode(), &[], &NoReferences)?;
        let tags = read_podcast_tags(&path)?;

        let json = tags.chapters_json.as_deref().expect("chapters frame");
        let value: serde_json::Value = serde_json::from_str(json)?;
        let entries = value.as_array().expect("array");
        assert_eq!(entries[0]["value_routes"], song_routes(), "{name}");
        let remote_without_routes = entries[1].as_object().expect("object");
        assert_eq!(
            remote_without_routes.get("value_routes"),
            Some(&serde_json::Value::Null),
            "{name}"
        );
        assert_eq!(
            entries[2]["value_routes"],
            json!([{"address": "host@example.com", "split": 100, "type": "lnaddress"}]),
            "{name}"
        );

        assert_eq!(tags.chapters()?, expected_entries(), "{name}");
    }
    Ok(())
}

/// Musicindex ADR 0099 R-IMAGE-4: a remote entry holds the image URL of its
/// song. A remote split with no URL and a local split give JSON `null`. The
/// `CHAP` frames get no image subframe (R-IMAGE-4 item 7).
#[test]
fn entry_holds_the_image_url_of_its_song_or_null() -> Result<()> {
    let dir = TempDir::new()?;
    for name in ["old-frames.mp3", "old-frames.flac"] {
        let path = copy_fixture(&dir, name);
        write_episode_tags(&path, &episode(), &[], &NoReferences)?;
        let tags = read_podcast_tags(&path)?;
        let json = tags.chapters_json.as_deref().expect("chapters frame");
        let value: serde_json::Value = serde_json::from_str(json)?;
        let entries = value.as_array().expect("array");
        assert_eq!(entries[0]["image_url"], SONG_IMAGE, "{name}");
        for entry in &entries[1..] {
            let entry = entry.as_object().expect("object");
            assert_eq!(
                entry.get("image_url"),
                Some(&serde_json::Value::Null),
                "{name}"
            );
        }
        let images = tags.chapters()?.into_iter().map(|entry| entry.image_url);
        assert_eq!(
            images.collect::<Vec<_>>(),
            vec![Some(SONG_IMAGE.to_owned()), None, None],
            "{name}"
        );
    }
    let path = copy_fixture(&dir, "old-frames.mp3");
    write_episode_tags(&path, &episode(), &[], &NoReferences)?;
    let tag = Tag::read_from_path(&path)?;
    assert_eq!(tag.chapters().count(), 3);
    for chapter in tag.chapters() {
        let subframes = chapter
            .frames
            .iter()
            .map(|frame| {
                let label = frame
                    .content()
                    .extended_text()
                    .map(|text| text.description.clone());
                (frame.id().to_owned(), label)
            })
            .collect::<Vec<_>>();
        assert!(
            subframes.iter().all(|(id, label)| {
                matches!(id.as_str(), "TIT2" | "TXXX")
                    && !label
                        .as_deref()
                        .is_some_and(|label| label.contains("Image"))
            }),
            "musicindex ADR 0099 R-IMAGE-4 item 7: a CHAP frame gets no image subframe: {subframes:?}"
        );
    }
    Ok(())
}

/// Musicindex ADR 0099 R-IMAGE-5: an entry from before ADR 0099 has no
/// `image_url` and reads as `None` (item 1). An entry with a field that the
/// reader does not know reads with no error (item 3).
#[test]
fn reader_accepts_a_missing_image_url_and_an_unknown_field() -> Result<()> {
    let old = PodcastTags {
        chapters_json: Some(
            r#"[{"start_ms":0,"end_ms":1000,"title":"Song","artist":null,"feed_guid":"feed","item_guid":null,"remote_percentage":100,"value_routes":null}]"#
                .into(),
        ),
        ..PodcastTags::default()
    };
    let entries = old.chapters()?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].image_url, None);
    assert_eq!(entries[0].title.as_deref(), Some("Song"));

    let later = PodcastTags {
        chapters_json: Some(
            r#"[{"start_ms":0,"end_ms":1000,"title":null,"artist":null,"feed_guid":"feed","item_guid":null,"remote_percentage":100,"value_routes":null,"image_url":"https://example.com/a.jpg","later_field":{"any":[1,2]}}]"#
                .into(),
        ),
        ..PodcastTags::default()
    };
    let entries = later.chapters()?;
    assert_eq!(
        entries[0].image_url.as_deref(),
        Some("https://example.com/a.jpg")
    );
    Ok(())
}

/// R-TAG-1 item 5: a FLAC episode gets the chapter JSON and no chapter frame.
#[test]
fn episode_flac_gets_the_json_only() -> Result<()> {
    let dir = TempDir::new()?;
    let path = copy_fixture(&dir, "old-frames.flac");
    write_episode_tags(&path, &episode(), &[], &NoReferences)?;
    let tags = read_podcast_tags(&path)?;
    assert_eq!(tags.feed_guid.as_deref(), Some("show-feed-guid"));
    assert_eq!(tags.chapters()?, expected_entries());
    assert!(tags.chapter_frames.is_empty());
    assert!(tags.toc.is_empty());
    Ok(())
}

/// R-TAG-4 item 3: an episode with no split gets no chapter frame. The write
/// also removes the chapter frames of an earlier write.
#[test]
fn episode_with_no_split_has_no_chapter_frame() -> Result<()> {
    let dir = TempDir::new()?;
    for name in ["old-frames.mp3", "old-frames.flac"] {
        let path = copy_fixture(&dir, name);
        write_episode_tags(&path, &episode(), &[], &NoReferences)?;
        let mut empty = episode();
        empty.splits.clear();
        write_episode_tags(&path, &empty, &[], &NoReferences)?;

        let tags = read_podcast_tags(&path)?;
        assert_eq!(tags.feed_guid.as_deref(), Some("show-feed-guid"), "{name}");
        assert!(tags.chapters_json.is_none(), "{name}");
        assert!(tags.chapter_frames.is_empty(), "{name}");
        assert!(tags.toc.is_empty(), "{name}");
        assert!(
            !tags
                .audio
                .fields
                .iter()
                .any(|field| matches!(field.frame_id.as_str(), "CHAP" | "CTOC")),
            "{name}"
        );
    }
    Ok(())
}

/// R-TAG-4 item 3: 256 splits give an error, and the file does not change.
#[test]
fn episode_with_256_splits_is_an_error_and_writes_nothing() -> Result<()> {
    let dir = TempDir::new()?;
    let path = copy_fixture(&dir, "old-frames.mp3");
    let before = std::fs::read(&path)?;
    let mut many = episode();
    many.splits = (0..256)
        .map(|index| {
            split(
                f64::from(index) * 10.0,
                10.0,
                Some("Song"),
                remote("feed", Some("item"), None),
            )
        })
        .collect();

    let error = write_episode_tags(&path, &many, &[], &NoReferences).expect_err("256 splits");
    assert_eq!(
        error.downcast_ref::<TooManyChapters>(),
        Some(&TooManyChapters { count: 256 })
    );
    assert_eq!(std::fs::read(&path)?, before);

    many.splits.truncate(255);
    write_episode_tags(&path, &many, &[], &NoReferences)?;
    assert_eq!(read_podcast_tags(&path)?.toc.len(), 255);
    Ok(())
}

/// R-TAG-2 item 5: after a write, the file has the new frames and no old
/// frame.
#[test]
fn write_removes_each_old_frame_of_an_mp3_file() -> Result<()> {
    let dir = TempDir::new()?;
    let path = mp3_with_each_old_frame(&dir);
    assert_eq!(old_id3_labels(&path).len(), 8);

    let result = write_song_tags(&path, &song(), &[], &NoReferences)?;

    assert!(old_id3_labels(&path).is_empty());
    let mut removed = result.removed_old_frames;
    removed.sort();
    assert_eq!(
        removed,
        vec![
            "COMM:MusicIndex Album Description",
            "SYLT:MusicIndex Transcript",
            "TXXX:MusicIndex Contributors",
            "TXXX:MusicIndex Feed Guid",
            "TXXX:MusicIndex Track Guid",
            "TXXX:MusicIndex Transcript",
            "TXXX:MusicIndex Value Routes",
            "USLT:MusicIndex Transcript",
        ]
    );
    let tags = read_podcast_tags(&path)?;
    assert_eq!(tags.feed_guid.as_deref(), Some("song-feed-guid"));
    // A frame with no old name stays.
    assert!(
        tags.audio
            .fields
            .iter()
            .any(|field| field.frame_id == "TXXX:V4V_PUBLISHER")
    );
    Ok(())
}

/// R-TAG-2 item 5, for a FLAC file: each Vorbis comment with an old key goes.
#[test]
fn write_removes_each_old_key_of_a_flac_file() -> Result<()> {
    let dir = TempDir::new()?;
    let path = copy_fixture(&dir, "old-frames.flac");
    let result = write_song_tags(&path, &song(), &[], &NoReferences)?;

    assert_eq!(result.removed_old_frames.len(), 5);
    let tags = read_podcast_tags(&path)?;
    assert!(
        !tags
            .audio
            .fields
            .iter()
            .any(|field| frames::is_old_description(frames::descriptor(&field.frame_id))),
        "{:?}",
        tags.audio.fields
    );
    assert_eq!(tags.item_guid.as_deref(), Some("song-item-guid"));
    Ok(())
}

/// R-TAG-2 item 2: the reader does not accept an old name.
#[test]
fn reader_ignores_old_names() -> Result<()> {
    for name in ["old-frames.mp3", "old-frames.flac"] {
        let tags = read_podcast_tags(&fixture(name))?;
        assert_eq!(tags.feed_guid, None, "{name}");
        assert_eq!(tags.item_guid, None, "{name}");
        assert_eq!(tags.value_routes, None, "{name}");
        assert_eq!(tags.contributors, None, "{name}");
    }
    Ok(())
}

/// The retag of a collection: a second write of the same tags gives the same
/// bytes.
#[test]
fn second_write_gives_the_same_bytes() -> Result<()> {
    let dir = TempDir::new()?;
    let mp3 = mp3_with_each_old_frame(&dir);
    let flac = copy_fixture(&dir, "old-frames.flac");
    let edits = [
        Id3v24Edit {
            frame_label: "TIT2".into(),
            value: "Episode Title".into(),
        },
        Id3v24Edit {
            frame_label: frames::DESCRIPTION.into(),
            value: "Episode description".into(),
        },
        Id3v24Edit {
            frame_label: frames::FEED_DESCRIPTION.into(),
            value: "Show description".into(),
        },
    ];
    for path in [mp3, flac] {
        write_episode_tags(&path, &episode(), &edits, &NoReferences)?;
        let first = std::fs::read(&path)?;
        let second_result = write_episode_tags(&path, &episode(), &edits, &NoReferences)?;
        assert_eq!(std::fs::read(&path)?, first, "{}", path.display());
        assert!(second_result.removed_old_frames.is_empty());

        write_song_tags(&path, &song(), &[], &NoReferences)?;
        let song_bytes = std::fs::read(&path)?;
        write_song_tags(&path, &song(), &[], &NoReferences)?;
        assert_eq!(std::fs::read(&path)?, song_bytes, "{}", path.display());
    }
    Ok(())
}

/// R-TAG-3 and R-TAG-4: an empty GUID is an error, and the file does not
/// change.
#[test]
fn empty_guid_is_an_error() -> Result<()> {
    let dir = TempDir::new()?;
    let path = copy_fixture(&dir, "old-frames.mp3");
    let before = std::fs::read(&path)?;
    let mut empty = song();
    empty.item_guid = " ".into();
    assert!(write_song_tags(&path, &empty, &[], &NoReferences).is_err());
    assert_eq!(std::fs::read(&path)?, before);
    Ok(())
}

/// R-TAG-2 item 5: a write with no edit still removes the old frames, for
/// example when an old route frame has no new value. A second such write
/// does not change the file.
#[test]
fn write_with_no_edit_removes_the_old_frames() -> Result<()> {
    let dir = TempDir::new()?;
    let mp3 = mp3_with_each_old_frame(&dir);
    let flac = copy_fixture(&dir, "old-frames.flac");
    for (path, count) in [(mp3, 8), (flac, 5)] {
        let result = write_id3v24_edits(&path, &[], &NoReferences)?;
        assert_eq!(result.removed_old_frames.len(), count, "{}", path.display());
        assert_eq!(result.applied, 0);
        let tags = read_podcast_tags(&path)?;
        assert!(
            !tags
                .audio
                .fields
                .iter()
                .any(|field| frames::is_old_description(frames::descriptor(&field.frame_id))),
            "{:?}",
            tags.audio.fields
        );

        let bytes = std::fs::read(&path)?;
        let second = write_id3v24_edits(&path, &[], &NoReferences)?;
        assert!(second.removed_old_frames.is_empty());
        assert_eq!(std::fs::read(&path)?, bytes, "{}", path.display());
    }
    Ok(())
}

/// A file with no old frame and no edit does not change: the writer does not
/// write it.
#[test]
fn write_with_no_edit_and_no_old_frame_does_not_write() -> Result<()> {
    let dir = TempDir::new()?;
    let flac = copy_fixture(&dir, "untagged.flac");
    let mp3 = copy_fixture(&dir, "old-frames.mp3");
    write_song_tags(&mp3, &song(), &[], &NoReferences)?;
    for path in [mp3, flac] {
        let bytes = std::fs::read(&path)?;
        let modified = std::fs::metadata(&path)?.modified()?;
        let result = write_id3v24_edits(&path, &[], &NoReferences)?;
        assert!(result.removed_old_frames.is_empty(), "{}", path.display());
        assert_eq!(std::fs::read(&path)?, bytes, "{}", path.display());
        assert_eq!(
            std::fs::metadata(&path)?.modified()?,
            modified,
            "{}",
            path.display()
        );
    }
    Ok(())
}

/// musicindex ADR 0096 R-IMAGE-2: a song write and an episode write with an
/// image URL give `TXXX:Podcast Image` with that URL, in MP3 and FLAC. A
/// write with no image URL gives no frame.
#[test]
fn the_image_url_is_written_only_when_given() -> Result<()> {
    const IMAGE: &str = "https://example.com/art/cover.jpg";
    let dir = TempDir::new()?;
    for name in ["old-frames.mp3", "old-frames.flac"] {
        let path = copy_fixture(&dir, name);
        write_song_tags(&path, &song(), &[], &NoReferences)?;
        assert_eq!(read_podcast_tags(&path)?.image, None, "{name}: song");
        write_episode_tags(&path, &episode(), &[], &NoReferences)?;
        assert_eq!(read_podcast_tags(&path)?.image, None, "{name}: episode");

        let path = copy_fixture(&dir, name);
        let with_image = SongTags {
            image: Some(IMAGE.into()),
            ..song()
        };
        write_song_tags(&path, &with_image, &[], &NoReferences)?;
        assert_eq!(
            read_podcast_tags(&path)?.image.as_deref(),
            Some(IMAGE),
            "musicindex ADR 0096 R-IMAGE-2 item 2: {name}: song"
        );

        let path = copy_fixture(&dir, name);
        let with_image = EpisodeTags {
            image: Some(IMAGE.into()),
            ..episode()
        };
        write_episode_tags(&path, &with_image, &[], &NoReferences)?;
        assert_eq!(
            read_podcast_tags(&path)?.image.as_deref(),
            Some(IMAGE),
            "musicindex ADR 0096 R-IMAGE-2 item 2: {name}: episode"
        );
    }
    Ok(())
}
