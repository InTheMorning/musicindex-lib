//! The kind of a release (musicindex ADR 0020 R-WORD-1, ADR 0049 R-QUERY-6).
//!
//! The source is `src/view_models/release_kind.rs` of v4vmm. A release is an
//! Album, an EP or a Single. The stated value from the feed wins when it names
//! one of the three. Without a stated value, the track count decides.
//!
//! [`ReleaseKind::track_counts`] is the one place of the count rule. The label
//! and the Index request both read it, so a change there changes both. A test
//! in this file fails when the label and the request do not agree.

use crate::query::KindValue;

/// The value of `kind` that finds a feed with no stated kind (Stophammer ADR 0085
/// section 6).
const UNSTATED_KIND: &str = "unknown";

/// The track counts of an inferred kind. Each limit is inclusive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TrackCounts {
    /// The lowest track count.
    min: usize,
    /// The highest track count. `None` gives no limit.
    max: Option<usize>,
}

impl TrackCounts {
    /// `true` when `count` is in the range.
    fn contains(self, count: usize) -> bool {
        count >= self.min && self.max.is_none_or(|max| count <= max)
    }

    /// The range from the lowest limit of the two ranges to their highest limit.
    fn joined(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.zip(other.max).map(|(a, b)| a.max(b)),
        }
    }
}

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
    /// Without a stated value, [`ReleaseKind::track_counts`] decides. A count
    /// that no kind holds, such as zero, gives an Album.
    #[must_use]
    pub fn resolve(stated: Option<&str>, track_count: usize) -> Self {
        if let Some(kind) = stated.map(str::trim).and_then(Self::from_stated_value) {
            return kind;
        }
        [Self::Single, Self::Ep, Self::Album]
            .into_iter()
            .find(|kind| {
                kind.track_counts()
                    .is_some_and(|counts| counts.contains(track_count))
            })
            .unwrap_or(Self::Album)
    }

    /// The count rule: the track counts that give this kind when the feed
    /// states no kind (musicindex ADR 0049 R-QUERY-6). `None` means that no count
    /// gives this kind, so `kind:` sends no Index request for it. A test of EP
    /// settings changes this function only.
    const fn track_counts(self) -> Option<TrackCounts> {
        match self {
            Self::Album => Some(TrackCounts { min: 2, max: None }),
            Self::Ep => None,
            Self::Single => Some(TrackCounts {
                min: 1,
                max: Some(1),
            }),
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

/// The Index parameters of the `kind:` values `kinds` (musicindex ADR 0049
/// R-QUERY-6). `None` means that the values send no Index request. The result
/// holds each track count from the lowest limit to the highest limit, so the
/// count rule must give a range with no gap for each set of values.
#[must_use]
pub fn index_kind_pairs(kinds: &[KindValue]) -> Option<Vec<(&'static str, String)>> {
    let counts = kinds
        .iter()
        .filter_map(|kind| ReleaseKind::from_query_kind(*kind).track_counts())
        .reduce(TrackCounts::joined)?;
    let mut pairs = vec![("kind", UNSTATED_KIND.to_owned())];
    if counts.max == Some(counts.min) {
        pairs.push(("tracks", counts.min.to_string()));
    } else {
        pairs.push(("min_tracks", counts.min.to_string()));
        if let Some(max) = counts.max {
            pairs.push(("max_tracks", max.to_string()));
        }
    }
    Some(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R7-01: a stated value wins over the track count, after trim and
    /// read without case. An unstated value falls back to the count rule,
    /// and a track count never gives an EP.
    #[test]
    fn adr_0084_release_kind_r7_01_resolve_follows_decision_5() {
        assert_eq!(ReleaseKind::resolve(Some("Album"), 1), ReleaseKind::Album);
        assert_eq!(ReleaseKind::resolve(Some("ep"), 1), ReleaseKind::Ep);
        assert_eq!(
            ReleaseKind::resolve(Some(" Single "), 5),
            ReleaseKind::Single
        );

        assert_eq!(
            ReleaseKind::resolve(Some("unknown"), 1),
            ReleaseKind::Single
        );
        assert_eq!(ReleaseKind::resolve(Some("unknown"), 5), ReleaseKind::Album);

        assert_eq!(ReleaseKind::resolve(None, 0), ReleaseKind::Album);
        assert_eq!(ReleaseKind::resolve(None, 12), ReleaseKind::Album);
    }

    /// R87-2-06: each `kind:` value gives its release kind.
    #[test]
    fn adr_0087_library_fields_r87_2_06_query_kind_gives_the_release_kind() {
        assert_eq!(
            ReleaseKind::from_query_kind(KindValue::Album),
            ReleaseKind::Album
        );
        assert_eq!(ReleaseKind::from_query_kind(KindValue::Ep), ReleaseKind::Ep);
        assert_eq!(
            ReleaseKind::from_query_kind(KindValue::Single),
            ReleaseKind::Single
        );
    }

    /// R7-01: the visible label and the action noun of each kind.
    #[test]
    fn adr_0084_release_kind_label_and_noun() {
        assert_eq!(ReleaseKind::Album.label(), "Album");
        assert_eq!(ReleaseKind::Ep.label(), "EP");
        assert_eq!(ReleaseKind::Single.label(), "Single");

        assert_eq!(ReleaseKind::Album.noun(), "album");
        assert_eq!(ReleaseKind::Ep.noun(), "EP");
        assert_eq!(ReleaseKind::Single.noun(), "single");
    }

    /// The requests of musicindex ADR 0049 R-QUERY-6.
    #[test]
    fn adr_0049_r_query_6_each_set_of_kinds_gives_its_request() {
        let pairs = |name: &'static str, value: &str| {
            Some(vec![
                ("kind", "unknown".to_owned()),
                (name, value.to_owned()),
            ])
        };
        let (album, ep, single) = (KindValue::Album, KindValue::Ep, KindValue::Single);
        assert_eq!(index_kind_pairs(&[]), None);
        assert_eq!(index_kind_pairs(&[ep]), None);
        assert_eq!(index_kind_pairs(&[single]), pairs("tracks", "1"));
        assert_eq!(index_kind_pairs(&[album]), pairs("min_tracks", "2"));
        assert_eq!(index_kind_pairs(&[album, single]), pairs("min_tracks", "1"));
        assert_eq!(
            index_kind_pairs(&[album, ep, single]),
            pairs("min_tracks", "1")
        );
        assert_eq!(index_kind_pairs(&[ep, single]), pairs("tracks", "1"));
        assert_eq!(index_kind_pairs(&[ep, album]), pairs("min_tracks", "2"));
    }

    /// The label and the request stay equal (musicindex ADR 0049 R-QUERY-6 item 2).
    /// A count from 1 is in the range of a request when the label of that count
    /// is one of its kinds. A count of zero is the known difference of item 1.
    #[test]
    fn adr_0049_r_query_6_the_label_and_the_request_agree() {
        let all = [KindValue::Album, KindValue::Ep, KindValue::Single];
        for mask in 1..8_usize {
            let kinds: Vec<KindValue> = all
                .iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) != 0)
                .map(|(_, kind)| *kind)
                .collect();
            let counts = kinds
                .iter()
                .filter_map(|kind| ReleaseKind::from_query_kind(*kind).track_counts())
                .reduce(TrackCounts::joined);
            for count in 1..=200 {
                let label = ReleaseKind::resolve(None, count);
                let labeled = kinds
                    .iter()
                    .any(|kind| ReleaseKind::from_query_kind(*kind) == label);
                let requested = counts.is_some_and(|range| range.contains(count));
                assert_eq!(
                    labeled, requested,
                    "musicindex ADR 0049 R-QUERY-6: {kinds:?} and {count} tracks give the label \
                     {label:?}. Change ReleaseKind::track_counts so each set of kinds gives one \
                     range with no gap, or change the request of index_kind_pairs."
                );
            }
        }
    }
}
