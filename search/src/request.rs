//! The parameters of one Index request (musicindex ADR 0049 R-QUERY-4).
//!
//! The source is `index_filter_pairs` of `src/application/queries/search.rs` of
//! v4vmm. An app sends these parameters with its own HTTP code.

use crate::kind::index_kind_pairs;
use crate::plan::IndexFilters;

/// The `GET /v1/search` filter parameters of one request (v4vmm ADR 0087
/// Decision 5, Stophammer ADR 0080 section 2). Each field is its own
/// parameter. A date is `YYYY-MM-DD`, and a duration is whole seconds.
#[must_use]
pub fn index_filter_pairs(filters: &IndexFilters) -> Vec<(&'static str, String)> {
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    let texts = [
        ("artist", &filters.artist),
        ("title", &filters.title),
        ("release", &filters.release),
    ];
    for (name, value) in texts {
        if let Some(value) = value {
            pairs.push((name, value.clone()));
        }
    }
    for (name, values) in [
        ("not_artist", &filters.not_artist),
        ("not_title", &filters.not_title),
        ("not_release", &filters.not_release),
        ("not_q", &filters.not_q),
    ] {
        pairs.extend(values.iter().map(|value| (name, value.clone())));
    }
    pairs.extend(index_kind_pairs(&filters.kinds).into_iter().flatten());
    if let Some(day) = filters.after {
        pairs.push(("after", day.format("%Y-%m-%d").to_string()));
    }
    if let Some(day) = filters.before {
        pairs.push(("before", day.format("%Y-%m-%d").to_string()));
    }
    if let Some(secs) = filters.min_duration {
        pairs.push(("min_duration", secs.to_string()));
    }
    if let Some(secs) = filters.max_duration {
        pairs.push(("max_duration", secs.to_string()));
    }
    pairs
}
