//! The frame names of musicindex ADR 0101 R-TAG-2.
//!
//! A writer writes only these names. A reader reads only these names. The
//! value form of each frame is the value form of its old name.

/// The `podcast:guid` of the feed (R-TAG-3 item 1, R-TAG-4 item 1).
pub const FEED_GUID: &str = "TXXX:Podcast Feed Guid";
/// The `guid` of the item (R-TAG-3 item 2, R-TAG-4 item 1).
pub const ITEM_GUID: &str = "TXXX:Podcast Item Guid";
/// The value block of the item, as a JSON array of recipients.
pub const VALUE_ROUTES: &str = "TXXX:Podcast Value Routes";
/// The image URL of the item.
pub const IMAGE: &str = "TXXX:Podcast Image";
/// The persons of the item.
pub const CONTRIBUTORS: &str = "TXXX:Podcast Contributors";
/// The text of the license, for example `cc-by-4.0`.
pub const LICENSE: &str = "TXXX:Podcast License";
/// The description of the item.
pub const DESCRIPTION: &str = "COMM:Podcast Description";
/// The description of the feed.
pub const FEED_DESCRIPTION: &str = "COMM:Podcast Feed Description";
/// The transcript as plain text.
pub const TRANSCRIPT_TEXT: &str = "USLT:Podcast Transcript";
/// The transcript with a time for each line.
pub const TRANSCRIPT_TIMED: &str = "SYLT:Podcast Transcript";
/// The chapter songs of an episode as a JSON array (R-TAG-4 item 4).
pub const CHAPTERS: &str = "TXXX:Podcast Chapters";

/// Each frame name of R-TAG-2.
pub const PODCAST_FRAMES: [&str; 11] = [
    FEED_GUID,
    ITEM_GUID,
    VALUE_ROUTES,
    IMAGE,
    CONTRIBUTORS,
    LICENSE,
    DESCRIPTION,
    FEED_DESCRIPTION,
    TRANSCRIPT_TEXT,
    TRANSCRIPT_TIMED,
    CHAPTERS,
];

/// The start of the description of each old frame (R-TAG-2 item 5).
pub const OLD_DESCRIPTION_PREFIX: &str = "MusicIndex ";

/// The frame IDs of R-TAG-2 item 5. A frame with one of these IDs and an old
/// description is an old frame.
pub const OLD_FRAME_IDS: [&str; 4] = ["TXXX", "COMM", "USLT", "SYLT"];

/// The part of `label` after the frame ID, for example `Podcast Feed Guid`
/// for `TXXX:Podcast Feed Guid`. A label with no descriptor gives itself.
#[must_use]
pub fn descriptor(label: &str) -> &str {
    label
        .split_once(':')
        .map_or(label, |(_, descriptor)| descriptor)
}

/// `true` when `description` starts with `MusicIndex `, with any letter case
/// (R-TAG-2 item 5). A Vorbis comment holds the old key in capital letters.
#[must_use]
pub fn is_old_description(description: &str) -> bool {
    description
        .get(..OLD_DESCRIPTION_PREFIX.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(OLD_DESCRIPTION_PREFIX))
}

/// `true` for an old frame of R-TAG-2 item 5: a `TXXX`, `COMM`, `USLT` or
/// `SYLT` frame with a description that starts with `MusicIndex `.
#[must_use]
pub fn is_old_frame(frame_id: &str, description: &str) -> bool {
    OLD_FRAME_IDS.contains(&frame_id) && is_old_description(description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_frame_has_one_of_four_ids_and_the_old_prefix() {
        assert!(is_old_frame("TXXX", "MusicIndex Track Guid"));
        assert!(is_old_frame("COMM", "MusicIndex Album Description"));
        assert!(is_old_frame("USLT", "MusicIndex Transcript"));
        assert!(is_old_frame("SYLT", "MusicIndex Transcript"));
        assert!(is_old_frame("TXXX", "MUSICINDEX FEED GUID"));
        assert!(!is_old_frame("WXXX", "MusicIndex Feed Guid"));
        assert!(!is_old_frame("TXXX", "Podcast Feed Guid"));
        assert!(!is_old_frame("TXXX", "MusicIndexFeed"));
        assert!(!is_old_frame("TXXX", "V4V_PUBLISHER"));
    }

    #[test]
    fn no_new_frame_is_an_old_frame() {
        for label in PODCAST_FRAMES {
            let (frame_id, description) = label.split_once(':').expect("descriptor");
            assert!(!is_old_frame(frame_id, description), "{label}");
        }
    }
}
