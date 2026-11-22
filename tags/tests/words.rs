//! Musicindex ADR 0087 R-WORD-14: a test finds a music word in a frame name.

use musicindex_tags::{ChapterEntry, frames};
use serde_json::json;

/// The music words of R-WORD-14 item 1, with the namespace term of their
/// glossary row in musicindex `docs/glossary.md`.
const MUSIC_WORDS: [(&str, &str); 4] = [
    ("Album", "Feed"),
    ("Track", "Item"),
    ("Song", "Item"),
    ("Release", "Feed"),
];

/// The JSON fields of R-WORD-11 item 2. R-WORD-14 item 2 does not examine them.
const MUSIC_DATA_FIELDS: [&str; 2] = ["title", "artist"];

/// The failure message for `name`, or `None` when `name` holds no music word.
fn music_word_failure(name: &str) -> Option<String> {
    name.split(|ch: char| !ch.is_ascii_alphanumeric())
        .find_map(|word| {
            MUSIC_WORDS
                .iter()
                .find(|(music, _)| music.eq_ignore_ascii_case(word))
        })
        .map(|(music, term)| {
            format!(
                "musicindex ADR 0087 R-WORD-14: the name `{name}` holds the music word \
                 `{music}`. Use the namespace term `{term}` of the glossary row `{music}` in \
                 musicindex docs/glossary.md."
            )
        })
}

/// The field names of the chapter JSON, from an entry with each field set.
fn chapter_json_fields() -> Vec<String> {
    let entry = ChapterEntry {
        start_ms: 0,
        end_ms: 1,
        title: Some(String::new()),
        artist: Some(String::new()),
        author: Some(String::new()),
        feed_guid: Some(String::new()),
        item_guid: Some(String::new()),
        remote_percentage: 100,
        value_routes: Some(json!([])),
    };
    serde_json::to_value(entry)
        .expect("serialize entry")
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn no_frame_name_or_chapter_field_holds_a_music_word() {
    let mut examined = frames::PODCAST_FRAMES
        .iter()
        .map(|label| (*label).to_owned())
        .collect::<Vec<_>>();
    examined.extend(
        chapter_json_fields()
            .into_iter()
            .filter(|field| !MUSIC_DATA_FIELDS.contains(&field.as_str())),
    );
    assert_eq!(examined.len(), 11 + 7);

    let failures = examined
        .iter()
        .filter_map(|name| music_word_failure(name))
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// R-WORD-14 item 2: the music data fields give no failure.
#[test]
fn music_data_fields_give_no_failure() {
    for name in MUSIC_DATA_FIELDS
        .iter()
        .chain(&["TIT2", "TPE1", "TALB", "TRCK", "TPOS"])
    {
        assert_eq!(music_word_failure(name), None, "{name}");
    }
    let fields = chapter_json_fields();
    for name in MUSIC_DATA_FIELDS {
        assert!(fields.iter().any(|field| field == name), "{name}");
    }
}

/// The incident of R-WORD-14 item 1 fails, and the message names the rule and
/// the glossary row.
#[test]
fn the_incident_name_fails() {
    let failure = music_word_failure("TXXX:MusicIndex Track Guid").expect("a failure");
    assert!(failure.contains("R-WORD-14"));
    assert!(failure.contains("glossary row `Track`"));
    assert!(failure.contains("`Item`"));
    assert!(music_word_failure("COMM:MusicIndex Album Description").is_some());
}
