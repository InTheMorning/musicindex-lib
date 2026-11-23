//! Reads and writes the tags of song files and episode files for the suite
//! (musicindex ADR 0093 R-TAG-1).
//!
//! The crate uses `id3` for MP3 and `lofty` for other formats (HitIt ADR 0004
//! and HitIt ADR 0008). It writes only the frame names of musicindex ADR 0093
//! R-TAG-2, and each write removes the old frames of R-TAG-2 item 5.
//!
//! - [`write_song_tags`] writes the GUIDs of a song (R-TAG-3).
//! - [`write_episode_tags`] writes the GUIDs, the value block and the chapter
//!   songs of an episode (R-TAG-4).
//! - [`write_id3v24_edits`] writes a list of frame edits.
//! - [`read_podcast_tags`] reads the podcast frames with the new names.
//! - [`read_audio_tags`] reads each tag of a file.
//! - [`track::read_tags`] reads the frame list of the live publisher.

mod audio;
pub mod chapters;
pub mod field;
pub mod format;
pub mod frames;
pub mod id3v2;
mod podcast;
mod source;
pub mod track;

pub use audio::{
    AudioTags, BadInvolvedPerson, EmbeddedArtwork, Id3Field, Id3v24Edit, InvolvedPersonCause,
    LICENSE_LIST_FRAME, LICENSE_NAME_FRAME, LICENSE_URL_FRAME, TagWriteResult,
    id3v24_edit_label_is_writable, involved_people_value, read_audio_tags, read_audio_tags_checked,
    sanitize_title_text, split_license_list, vorbis_storage_key, write_id3v24_edits,
};
pub use chapters::{ChapterEntry, ChapterFrame, Split, SplitTarget, TooManyChapters};
pub use id3v2::{BadFrame, BadFrameAction, BadTag, KnownBadFrame, TagProblems, read_id3v2};
pub use podcast::{
    EpisodeTags, PodcastTags, SongTags, read_podcast_tags, write_episode_tags, write_song_tags,
};
pub use source::{NoReferences, ReferenceSource};
