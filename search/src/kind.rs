//! The kind of a release (musicindex ADR 0020 R-WORD-1).
//!
//! The source is `src/view_models/release_kind.rs` of v4vmm. A release is an
//! Album, an EP or a Single. The stated value from the feed wins when it names
//! one of the three. Without a stated value, the track count decides: one track
//! gives a Single, and any other count gives an Album. A count never gives an
//! EP.

use crate::query::KindValue;

/// The kind of a release (v4vmm ADR 0084 Decision 5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseKind {
    Album,
    Ep,
    Single,
}

impl ReleaseKind {
    /// Resolves the kind of a release (v4vmm ADR 0084 Decision 5).
    ///
    /// `stated` is the free-text value that the feed states for its
    /// release kind, after trim, read without case. It wins when it names
    /// "album", "ep" or "single". Each other value, for example
    /// "unknown", counts as not stated.
    ///
    /// Without a stated value, `track_count` decides: one track gives a
    /// Single, and any other count, including zero, gives an Album. A
    /// track count never gives an EP.
    #[must_use]
    pub fn resolve(stated: Option<&str>, track_count: usize) -> Self {
        if let Some(kind) = stated.map(str::trim).and_then(Self::from_stated_value) {
            return kind;
        }
        if track_count == 1 {
            Self::Single
        } else {
            Self::Album
        }
    }

    /// Reads a stated release-kind value, read without case. Returns
    /// `None` for a value that names none of the three kinds.
    fn from_stated_value(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("album") {
            Some(Self::Album)
        } else if value.eq_ignore_ascii_case("ep") {
            Some(Self::Ep)
        } else if value.eq_ignore_ascii_case("single") {
            Some(Self::Single)
        } else {
            None
        }
    }

    /// The release kind of one `kind:` value of the search query (v4vmm ADR 0087
    /// Decision 2). The map is in this file, because the kind rule stays in one
    /// place (musicindex ADR 0049 R-QUERY-6).
    #[must_use]
    pub const fn from_query_kind(kind: KindValue) -> Self {
        match kind {
            KindValue::Album => Self::Album,
            KindValue::Ep => Self::Ep,
            KindValue::Single => Self::Single,
        }
    }

    /// The visible label: "Album", "EP" or "Single".
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Album => "Album",
            Self::Ep => "EP",
            Self::Single => "Single",
        }
    }

    /// The noun for an action label: "album", "EP" or "single".
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Album => "album",
            Self::Ep => "EP",
            Self::Single => "single",
        }
    }

    /// `true` for a Single (v4vmm ADR 0085 Decision 3).
    #[must_use]
    pub const fn is_single(self) -> bool {
        matches!(self, Self::Single)
    }
}
