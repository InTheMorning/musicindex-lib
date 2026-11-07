//! The plan of the Index part of one query (musicindex ADR 0049 R-QUERY-1).
//!
//! The source is the plan and the filters of
//! `src/view_models/search_results/search_run.rs` of v4vmm. The `q` of a
//! request holds only the words and the phrases of the query. Each Index field
//! is one filter of the request. Only [`IndexSearchPlan::for_query`] builds an
//! [`IndexSearchRequest`]. [`IndexSearchRequest::for_entity`] gives the request
//! of each entity type. When the Index part sends no request, the plan names
//! the cause.
//!
//! The module has no I/O.

use chrono::NaiveDate;

use crate::query::{
    FieldValue, FieldValueState, KindValue, QueryFieldName, QueryScope, SearchQuery, TermKind,
    quote_phrase,
};

/// The plan of the Index part of one query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IndexSearchPlan {
    /// Send the Index search with this request.
    Send(IndexSearchRequest),
    /// Send no Index request, for this cause.
    Skip(IndexSkip),
}

impl IndexSearchPlan {
    /// The plan of `query` (v4vmm ADR 0087 Decision 5). Each correct field gives
    /// one filter. Each word and phrase stays in `q`.
    #[must_use]
    pub fn for_query(query: &SearchQuery) -> IndexSearchPlan {
        match query.scope() {
            QueryScope::Conflict => return IndexSearchPlan::Skip(IndexSkip::Conflict),
            QueryScope::LibraryOnly => return IndexSearchPlan::Skip(IndexSkip::LibraryOnly),
            QueryScope::Everything | QueryScope::IndexOnly => {}
        }
        if let Some(terms) = UnsupportedIndexTerms::from_query(query) {
            return IndexSearchPlan::Skip(IndexSkip::Unsupported(terms));
        }
        let Some(filters) = IndexFilters::from_query(query) else {
            return IndexSearchPlan::Skip(IndexSkip::NoIndexPart);
        };
        let request = IndexSearchRequest {
            q: query.index_q(),
            filters: Box::new(filters),
        };
        if request.has_positive_term() {
            IndexSearchPlan::Send(request)
        } else {
            IndexSearchPlan::Skip(IndexSkip::NoIndexPart)
        }
    }
}

/// The request of one Index search. Only [`IndexSearchPlan::for_query`]
/// builds it. `q` holds the words and the phrases. `filters` holds each
/// field as its own parameter. No field syntax reaches `q`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexSearchRequest {
    /// The words and the phrases of the query, or `None` when it has none.
    pub q: Option<String>,
    /// The filters of the query.
    pub filters: Box<IndexFilters>,
}

impl IndexSearchRequest {
    /// A request with the words `q` and no filter. The tests of an app use it.
    #[must_use]
    pub fn words(q: &str) -> IndexSearchRequest {
        IndexSearchRequest {
            q: Some(q.to_owned()),
            filters: Box::default(),
        }
    }

    /// `true` when the request has a word, a phrase or a positive filter.
    /// Stophammer gives `400` for a request that has exclusions only.
    #[must_use]
    pub fn has_positive_term(&self) -> bool {
        self.q.is_some() || self.filters.has_positive_field()
    }

    /// The request for one entity type, or `None` when that type cannot
    /// match the request. Stophammer gives `400` for a filter that the type
    /// does not take. A publisher row has no release, kind, date or
    /// duration. A feed row has no duration. A track takes each filter.
    #[must_use]
    pub fn for_entity(&self, entity: IndexEntity) -> Option<IndexSearchRequest> {
        let mut filters = self.filters.clone();
        match entity {
            IndexEntity::Track => {}
            IndexEntity::Feed => {
                if filters.has_duration() {
                    return None;
                }
            }
            IndexEntity::Publisher => {
                if filters.release.is_some()
                    || !filters.kinds.is_empty()
                    || filters.after.is_some()
                    || filters.before.is_some()
                    || filters.has_duration()
                {
                    return None;
                }
                filters.not_release.clear();
            }
        }
        let request = IndexSearchRequest {
            q: self.q.clone(),
            filters,
        };
        request.has_positive_term().then_some(request)
    }
}

/// The entity types that an app asks `GET /v1/search` for (v4vmm ADR 0087
/// Decision 5, v4vmm ADR 0084 packet 029).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexEntity {
    Feed,
    Track,
    Publisher,
}

/// The filters of one Index search. Each field is one parameter of
/// `GET /v1/search` (Stophammer ADR 0080 section 2). A text
/// value holds words and phrases, as `q` does. Each repeated positive text
/// term joins the others with a space, so each term applies.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexFilters {
    /// The text of `artist`.
    pub artist: Option<String>,
    /// The text of `title`.
    pub title: Option<String>,
    /// The text of `release`.
    pub release: Option<String>,
    /// The values of `kind`. They are an OR. Empty when no `kind:` applies.
    pub kinds: Vec<KindValue>,
    /// The first day of `after`, in UTC.
    pub after: Option<NaiveDate>,
    /// The last day of `before`, in UTC.
    pub before: Option<NaiveDate>,
    /// The shortest track duration, in seconds, of `min_duration`.
    pub min_duration: Option<u32>,
    /// The longest track duration, in seconds, of `max_duration`.
    pub max_duration: Option<u32>,
    /// The values of `not_artist`. Each value is one exclusion.
    pub not_artist: Vec<String>,
    /// The values of `not_title`. Each value is one exclusion.
    pub not_title: Vec<String>,
    /// The values of `not_release`. Each value is one exclusion.
    pub not_release: Vec<String>,
    /// The values of `not_q`. Each value is one excluded word or phrase.
    pub not_q: Vec<String>,
}

impl IndexFilters {
    /// The filters of `query`. A word or a phrase with `-` gives `not_q`.
    /// Each correct field gives its filter, except `in:`, the Library-only
    /// fields and an excluded `kind:`, `after:`, `before:` or `duration:`.
    /// [`IndexSearchPlan::for_query`] gives `Skip` for the last ones before
    /// this function runs. The result is `None` when the `kind:` terms share
    /// no kind, because no row can match them.
    #[must_use]
    pub fn from_query(query: &SearchQuery) -> Option<IndexFilters> {
        let mut filters = IndexFilters::default();
        for term in query.terms() {
            match &term.kind {
                TermKind::Word(text) if term.excluded => filters.not_q.push(text.clone()),
                TermKind::Phrase(text) if term.excluded => {
                    filters.not_q.push(quote_phrase(text));
                }
                TermKind::Word(_) | TermKind::Phrase(_) => {}
                TermKind::Field(field) => {
                    let FieldValueState::Valid(value) = &field.value else {
                        continue;
                    };
                    if !filters.add_field(field.name, value, term.excluded) {
                        return None;
                    }
                }
            }
        }
        Some(filters)
    }

    /// Adds one correct field. Returns `false` when the `kind:` terms share
    /// no kind. Each field that has no filter gives nothing.
    fn add_field(&mut self, name: QueryFieldName, value: &FieldValue, excluded: bool) -> bool {
        match (name, value, excluded) {
            (QueryFieldName::Artist, FieldValue::Text(text), false) => {
                join_text(&mut self.artist, text);
            }
            (QueryFieldName::Artist, FieldValue::Text(text), true) => {
                self.not_artist.push(index_text(text));
            }
            (QueryFieldName::Title, FieldValue::Text(text), false) => {
                join_text(&mut self.title, text);
            }
            (QueryFieldName::Title, FieldValue::Text(text), true) => {
                self.not_title.push(index_text(text));
            }
            (QueryFieldName::Release, FieldValue::Text(text), false) => {
                join_text(&mut self.release, text);
            }
            (QueryFieldName::Release, FieldValue::Text(text), true) => {
                self.not_release.push(index_text(text));
            }
            (QueryFieldName::Kind, FieldValue::Kinds(kinds), false) => {
                if self.kinds.is_empty() {
                    self.kinds.clone_from(kinds);
                } else {
                    self.kinds.retain(|kind| kinds.contains(kind));
                }
                return !self.kinds.is_empty();
            }
            (QueryFieldName::After, FieldValue::Date(date), false) => {
                self.after = Some(
                    self.after
                        .map_or(date.first_day, |day| day.max(date.first_day)),
                );
            }
            (QueryFieldName::Before, FieldValue::Date(date), false) => {
                self.before = Some(
                    self.before
                        .map_or(date.last_day, |day| day.min(date.last_day)),
                );
            }
            (QueryFieldName::Duration, FieldValue::Duration(range), false) => {
                self.min_duration = self.min_duration.into_iter().chain(range.min_secs).max();
                self.max_duration = self.max_duration.into_iter().chain(range.max_secs).min();
            }
            _ => {}
        }
        true
    }

    /// `true` when a filter is a word, a phrase, a `kind`, a date, a duration
    /// or a text filter without `-`.
    fn has_positive_field(&self) -> bool {
        self.artist.is_some()
            || self.title.is_some()
            || self.release.is_some()
            || !self.kinds.is_empty()
            || self.after.is_some()
            || self.before.is_some()
            || self.has_duration()
    }

    /// `true` when `min_duration` or `max_duration` applies.
    fn has_duration(&self) -> bool {
        self.min_duration.is_some() || self.max_duration.is_some()
    }
}

/// Adds one text value to a text filter. A second value joins the first one
/// with a space.
fn join_text(slot: &mut Option<String>, text: &str) {
    let text = index_text(text);
    *slot = Some(match slot.take() {
        Some(previous) => format!("{previous} {text}"),
        None => text,
    });
}

/// The form of one text value in a parameter: the text as it is, or in
/// double quotes when the text has a white space or a double quote.
fn index_text(text: &str) -> String {
    if text.chars().any(|ch| ch.is_whitespace() || ch == '"') {
        quote_phrase(text)
    } else {
        text.to_owned()
    }
}

/// The cause of an Index part that sends no request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IndexSkip {
    /// The Index has nothing to search. The query has no word, no phrase and
    /// no positive field, or its `kind:` terms share no kind.
    NoIndexPart,
    /// The query asks for the Library only.
    LibraryOnly,
    /// The query asks for the Library only and for the Index only.
    Conflict,
    /// The Index cannot apply one or more excluded fields of the query.
    Unsupported(UnsupportedIndexTerms),
}

/// The excluded fields that the Index cannot apply (v4vmm ADR 0087 Decision 5).
/// The contract has no `not_` form for `kind`, `after`, `before` or
/// `duration`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedIndexTerms {
    /// Each distinct token, in text order, for example `after:`.
    pub field_tokens: Vec<&'static str>,
}

impl UnsupportedIndexTerms {
    /// The excluded fields of `query` that the Index cannot apply. `None`
    /// when there are none.
    #[must_use]
    pub fn from_query(query: &SearchQuery) -> Option<Self> {
        let mut field_tokens: Vec<&'static str> = Vec::new();
        for term in query.terms() {
            let TermKind::Field(field) = &term.kind else {
                continue;
            };
            let unsupported = term.excluded
                && matches!(
                    field.name,
                    QueryFieldName::Kind
                        | QueryFieldName::After
                        | QueryFieldName::Before
                        | QueryFieldName::Duration
                )
                && matches!(field.value, FieldValueState::Valid(_));
            let token = field.name.info().token;
            if unsupported && !field_tokens.contains(&token) {
                field_tokens.push(token);
            }
        }
        (!field_tokens.is_empty()).then_some(Self { field_tokens })
    }

    /// The title of the note, for example "`MusicIndex` cannot exclude kind:
    /// and after: yet" (v4vmm ADR 0087 Decision 5).
    #[must_use]
    pub fn note_title(&self) -> String {
        let tokens = token_list(&self.field_tokens);
        if tokens.is_empty() {
            "MusicIndex cannot apply this search yet".to_owned()
        } else {
            format!("MusicIndex cannot exclude {tokens} yet")
        }
    }
}

/// Joins the tokens: `a`, `a and b`, or `a, b and c`.
fn token_list(tokens: &[&str]) -> String {
    match tokens {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [first @ .., last] => format!("{} and {last}", first.join(", ")),
    }
}
