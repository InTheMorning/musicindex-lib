//! The parameters of one Index request (musicindex ADR 0049 R-QUERY-4).
//!
//! The source is `index_filter_pairs` of `src/application/queries/search.rs` of
//! v4vmm. An app sends these parameters with its own HTTP code.

use crate::kind::index_kind_pairs;
use crate::plan::IndexFilters;

/// The name in the node capabilities that states `sort` and `reverse`
/// (Stophammer ADR 0084 section 11).
const SORT_CAPABILITY: &str = "sort";

/// The sequence of the rows of one search (Stophammer ADR 0084 sections 2 and 3).
///
/// `Relevance` has no direction, because the node gives `400` for a reversed
/// relevance. A cursor continues only the sequence that gave it (section 9), so
/// an app keeps one value for all pages of a run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IndexOrder {
    /// The default of the node: best match first with a word, and newest first
    /// with no word.
    #[default]
    Relevance,
    /// A to Z. With `reverse`, Z to A.
    Name { reverse: bool },
    /// Newest first. With `reverse`, oldest first.
    Date { reverse: bool },
}

/// What started a search, for the sequence of its rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexRun {
    /// A search that runs while a person types. Each page of `name` or `date`
    /// sorts its rows again, so a live search keeps `relevance` (Stophammer ADR
    /// 0084, Consequences).
    Live,
    /// A search that a person starts.
    Command,
}

/// The `sort` and `reverse` parameters of one search (Stophammer ADR 0084).
///
/// `capabilities` is the list `capabilities` of `GET /v1/node/capabilities`. A
/// node that does not list `sort` gets no parameter, because a node can ignore a
/// parameter with no error (section 11). `IndexOrder::Relevance` gets no
/// parameter, because a request with no word gives `400` for `sort=relevance`
/// (section 5), and the default of the node is the same sequence (section 4).
#[must_use]
pub fn index_order_pairs(
    order: IndexOrder,
    run: IndexRun,
    capabilities: &[impl AsRef<str>],
) -> Vec<(&'static str, String)> {
    let node_sorts = capabilities
        .iter()
        .any(|name| name.as_ref() == SORT_CAPABILITY);
    if !node_sorts || run == IndexRun::Live {
        return Vec::new();
    }
    let (sort, reverse) = match order {
        IndexOrder::Relevance => return Vec::new(),
        IndexOrder::Name { reverse } => ("name", reverse),
        IndexOrder::Date { reverse } => ("date", reverse),
    };
    let mut pairs = vec![("sort", sort.to_owned())];
    if reverse {
        pairs.push(("reverse", "true".to_owned()));
    }
    pairs
}

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
        ("host", &filters.host),
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
        ("not_host", &filters.not_host),
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
