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
        } else if request.filters.has_exclusion() {
            IndexSearchPlan::Skip(IndexSkip::ExclusionsOnly)
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

    /// `true` when a `not_` filter has a value.
    fn has_exclusion(&self) -> bool {
        !self.not_artist.is_empty()
            || !self.not_title.is_empty()
            || !self.not_release.is_empty()
            || !self.not_q.is_empty()
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
    /// The Index has nothing to search. The query has no word, no phrase, no
    /// positive field and no exclusion, or its `kind:` terms share no kind.
    NoIndexPart,
    /// The query has exclusions and no positive term. Stophammer needs a
    /// positive term. An app can show a note for this cause.
    ExclusionsOnly,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(text: &str) -> IndexSearchPlan {
        IndexSearchPlan::for_query(&SearchQuery::parse(text))
    }

    fn unsupported(field_tokens: Vec<&'static str>) -> IndexSearchPlan {
        IndexSearchPlan::Skip(IndexSkip::Unsupported(UnsupportedIndexTerms {
            field_tokens,
        }))
    }

    /// The request of a query that sends one.
    fn request(text: &str) -> IndexSearchRequest {
        match plan(text) {
            IndexSearchPlan::Send(request) => request,
            other @ IndexSearchPlan::Skip(_) => panic!("{text} must send a request, got {other:?}"),
        }
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("test date is correct")
    }

    /// R87-2-21: a Library-only field gives `Skip(LibraryOnly)`, also with
    /// `-`. `in:index` with a Library-only field gives `Skip(Conflict)`.
    #[test]
    fn adr_0087_library_fields_r87_2_21_scope_skips_the_index() {
        for text in [
            "haleen is:ready",
            "haleen -has:split",
            "haleen playlist:x",
            "in:library haleen",
        ] {
            assert_eq!(
                plan(text),
                IndexSearchPlan::Skip(IndexSkip::LibraryOnly),
                "R87-2-21: {text}"
            );
        }
        assert_eq!(
            plan("in:index is:ready"),
            IndexSearchPlan::Skip(IndexSkip::Conflict)
        );
    }

    /// R87-2-22: an excluded `kind:`, `after:`, `before:` or `duration:` gives
    /// `Skip(Unsupported)`. The contract has no `not_` form for them. Each
    /// distinct token gives one entry, in text order. `note_title` gives each
    /// text of the note of Decision 5.
    #[test]
    fn adr_0087_library_fields_r87_2_22_excluded_terms_the_index_cannot_express() {
        assert_eq!(plan("haleen -kind:single"), unsupported(vec!["kind:"]));
        assert_eq!(
            plan("haleen -after:2026-10 -kind:ep -after:2026-11"),
            unsupported(vec!["after:", "kind:"])
        );
        assert_eq!(plan("haleen -duration:>5m"), unsupported(vec!["duration:"]));
        assert_eq!(
            plan("haleen -before:2026-10-01"),
            unsupported(vec!["before:"])
        );

        let title = |tokens: Vec<&'static str>| {
            UnsupportedIndexTerms {
                field_tokens: tokens,
            }
            .note_title()
        };
        assert_eq!(title(vec!["kind:"]), "MusicIndex cannot exclude kind: yet");
        assert_eq!(
            title(vec!["kind:", "after:"]),
            "MusicIndex cannot exclude kind: and after: yet"
        );
        assert_eq!(
            title(vec!["after:", "before:", "duration:"]),
            "MusicIndex cannot exclude after:, before: and duration: yet"
        );
    }

    /// R87-2-23 (plan part): `q` holds only the words and the phrases of the
    /// query. A query with no word and no positive field sends nothing. `in:`
    /// never reaches `q`.
    #[test]
    fn adr_0087_library_fields_r87_2_23_send_holds_only_words() {
        let query = SearchQuery::parse("haleen \"night drive\"");
        assert_eq!(
            IndexSearchPlan::for_query(&query),
            IndexSearchPlan::Send(IndexSearchRequest {
                q: query.index_q(),
                filters: Box::default(),
            })
        );
        assert_eq!(
            plan("haleen in:index"),
            IndexSearchPlan::Send(IndexSearchRequest {
                q: Some("haleen".to_owned()),
                filters: Box::default(),
            })
        );
        assert_eq!(plan("is:"), IndexSearchPlan::Skip(IndexSkip::NoIndexPart));
    }

    /// R87-11-01: each correct field gives its own filter (v4vmm ADR 0087 Decisions
    /// 2 and 5). The words stay in `q`. A phrase keeps its double quotes.
    #[test]
    fn adr_0087_index_fields_r87_11_01_each_field_gives_its_filter() {
        let sent = request(
            r#"haleen artist:jsmovie title:"night drive" release:"aged friends" kind:single,ep after:2026-10-01 before:2026-10-31 duration:>5m"#,
        );
        assert_eq!(sent.q.as_deref(), Some("haleen"));
        assert_eq!(
            *sent.filters,
            IndexFilters {
                artist: Some("jsmovie".to_owned()),
                title: Some("\"night drive\"".to_owned()),
                release: Some("\"aged friends\"".to_owned()),
                kinds: vec![KindValue::Single, KindValue::Ep],
                after: Some(date(2026, 10, 1)),
                before: Some(date(2026, 10, 31)),
                min_duration: Some(301),
                ..IndexFilters::default()
            }
        );
    }

    /// R87-11-02: a query with only fields sends no `q`. A month in `after:`
    /// gives its first day, and a month in `before:` gives its last day.
    #[test]
    fn adr_0087_index_fields_r87_11_02_fields_only_send_no_q() {
        assert_eq!(
            request("kind:single"),
            IndexSearchRequest {
                q: None,
                filters: Box::new(IndexFilters {
                    kinds: vec![KindValue::Single],
                    ..IndexFilters::default()
                }),
            }
        );
        let sent = request("after:2026-10 artist:jsmovie");
        assert_eq!(sent.q, None);
        assert_eq!(sent.filters.after, Some(date(2026, 10, 1)));
        assert_eq!(sent.filters.before, None);
    }

    /// R87-11-03: an excluded word or phrase gives `not_q`. An excluded
    /// `artist:`, `title:` or `release:` gives its `not_` filter. Each value
    /// is one exclusion.
    #[test]
    fn adr_0087_index_fields_r87_11_03_exclusions_give_not_parameters() {
        let sent = request(
            r#"haleen -remix -"night drive" -artist:jsmovie -title:remix -release:"aged friends""#,
        );
        assert_eq!(sent.q.as_deref(), Some("haleen"));
        assert_eq!(
            *sent.filters,
            IndexFilters {
                not_artist: vec!["jsmovie".to_owned()],
                not_title: vec!["remix".to_owned()],
                not_release: vec!["\"aged friends\"".to_owned()],
                not_q: vec!["remix".to_owned(), "\"night drive\"".to_owned()],
                ..IndexFilters::default()
            }
        );
    }

    /// R87-11-04: the month and the duration forms convert to the values of
    /// the contract. The day of `before:` is the last day of the month, and a
    /// leap year keeps its 29 February. A duration gives whole seconds, each
    /// limit included.
    #[test]
    fn adr_0087_index_fields_r87_11_04_month_and_duration_conversion() {
        let filters = |text: &str| request(text).filters;
        assert_eq!(
            filters("haleen before:2028-02").before,
            Some(date(2028, 2, 29))
        );
        assert_eq!(
            filters("haleen after:2026-12").after,
            Some(date(2026, 12, 1))
        );
        assert_eq!(
            filters("haleen before:2026-12").before,
            Some(date(2026, 12, 31))
        );
        assert_eq!(
            filters("haleen after:2026-10-01").after,
            Some(date(2026, 10, 1))
        );
        let timed = filters("haleen duration:<3:30");
        assert_eq!((timed.min_duration, timed.max_duration), (None, Some(209)));
        let range = filters("haleen duration:3m..5m");
        assert_eq!(
            (range.min_duration, range.max_duration),
            (Some(180), Some(300))
        );
        let hours = filters("haleen duration:>=1h");
        assert_eq!((hours.min_duration, hours.max_duration), (Some(3600), None));
        let seconds = filters("haleen duration:<=90s");
        assert_eq!(
            (seconds.min_duration, seconds.max_duration),
            (None, Some(90))
        );
    }

    /// R87-11-05: `in:` gives no filter, and a Library-only field still sends
    /// nothing. A word with `in:index` and a field sends one request.
    #[test]
    fn adr_0087_index_fields_r87_11_05_scope_and_library_fields_give_no_filter() {
        let sent = request("haleen in:index artist:x");
        assert_eq!(sent.q.as_deref(), Some("haleen"));
        assert_eq!(sent.filters.artist.as_deref(), Some("x"));
        assert_eq!(
            plan("haleen is:ready"),
            IndexSearchPlan::Skip(IndexSkip::LibraryOnly)
        );
        assert_eq!(
            plan("in:library kind:ep"),
            IndexSearchPlan::Skip(IndexSkip::LibraryOnly)
        );
    }

    /// R87-11-06: several terms of one field. A second `artist:` joins the
    /// first one. The `after:` terms give the later day, and the `before:`
    /// terms give the earlier day. Two `duration:` terms give their common
    /// range. `kind:` terms share their kinds, and no shared kind gives no
    /// request.
    #[test]
    fn adr_0087_index_fields_r87_11_06_repeated_fields_combine() {
        let sent = request(
            r#"haleen artist:jsmovie artist:"night drive" after:2026-10-01 after:2026-10-05 before:2026-10-31 before:2026-10-03 duration:>5m duration:<10m"#,
        );
        assert_eq!(
            sent.filters.artist.as_deref(),
            Some("jsmovie \"night drive\"")
        );
        assert_eq!(sent.filters.after, Some(date(2026, 10, 5)));
        assert_eq!(sent.filters.before, Some(date(2026, 10, 3)));
        assert_eq!(
            (sent.filters.min_duration, sent.filters.max_duration),
            (Some(301), Some(599))
        );
        assert_eq!(
            request("kind:album,ep kind:ep").filters.kinds,
            vec![KindValue::Ep]
        );
        assert_eq!(
            plan("kind:single kind:ep"),
            IndexSearchPlan::Skip(IndexSkip::NoIndexPart)
        );
    }

    /// R87-11-07: a query with exclusions only sends nothing, because
    /// Stophammer needs a positive term. A word with exclusions sends one
    /// request.
    #[test]
    fn adr_0087_index_fields_r87_11_07_exclusions_alone_send_nothing() {
        assert_eq!(
            plan("-remix"),
            IndexSearchPlan::Skip(IndexSkip::ExclusionsOnly)
        );
        assert_eq!(
            plan("-title:remix"),
            IndexSearchPlan::Skip(IndexSkip::ExclusionsOnly)
        );
        assert_eq!(
            plan("-remix kind:single kind:ep"),
            IndexSearchPlan::Skip(IndexSkip::NoIndexPart)
        );
        assert!(matches!(
            plan("-title:remix haleen"),
            IndexSearchPlan::Send(_)
        ));
    }

    /// R87-11-08: `for_entity` gives the request of each entity type. A
    /// publisher row takes no release, kind, date or duration. A feed row
    /// takes no duration. A track takes each filter. A type with no positive
    /// term gets no request.
    #[test]
    fn adr_0087_index_fields_r87_11_08_each_entity_takes_its_filters() {
        let publisher =
            request("haleen artist:x title:y -release:z").for_entity(IndexEntity::Publisher);
        let publisher = publisher.expect("the publisher takes artist and title");
        assert_eq!(publisher.filters.artist.as_deref(), Some("x"));
        assert_eq!(publisher.filters.title.as_deref(), Some("y"));
        assert!(publisher.filters.not_release.is_empty());

        assert_eq!(
            request("haleen kind:single").for_entity(IndexEntity::Publisher),
            None
        );
        assert_eq!(
            request("haleen after:2026-10").for_entity(IndexEntity::Publisher),
            None
        );

        let timed = request("haleen duration:>5m");
        assert_eq!(timed.for_entity(IndexEntity::Feed), None);
        assert_eq!(timed.for_entity(IndexEntity::Publisher), None);
        let track = timed
            .for_entity(IndexEntity::Track)
            .expect("a track takes a duration");
        assert_eq!(track.filters.min_duration, Some(301));

        let feed = request("kind:single -title:x").for_entity(IndexEntity::Feed);
        assert_eq!(
            feed.expect("a feed takes a kind").filters.kinds,
            vec![KindValue::Single]
        );

        let exclusions_only = IndexSearchRequest {
            q: None,
            filters: Box::new(IndexFilters {
                not_title: vec!["x".to_owned()],
                ..IndexFilters::default()
            }),
        };
        assert_eq!(exclusions_only.for_entity(IndexEntity::Track), None);
    }

    /// R87-11-09: an excluded `kind:`, `after:`, `before:` or `duration:`
    /// gives `Skip(Unsupported)`, even when a word is in the query. A value
    /// that is not correct applies nothing, so it gives no skip.
    #[test]
    fn adr_0087_index_fields_r87_11_09_unsupported_before_the_filters() {
        assert_eq!(plan("haleen -kind:single"), unsupported(vec!["kind:"]));
        assert!(matches!(
            plan("haleen -after:yesterday"),
            IndexSearchPlan::Send(_)
        ));
    }
}
