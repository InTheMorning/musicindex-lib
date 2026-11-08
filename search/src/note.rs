//! The words of a note for a text that sends no Index request (musicindex ADR
//! 0052 R-QUERY-8).
//!
//! Each app shows these words, and it writes no words of its own for these
//! causes. An app with a Library adds one sentence about its Library rows.
//!
//! The module has no I/O.

use crate::field_list::Library;
use crate::plan::{IndexEntity, IndexSearchPlan, IndexSkip, refused_tokens, token_list};
use crate::query::SearchQuery;

/// The title and the text of one note.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchNote {
    /// The title of the note.
    pub title: String,
    /// The text under the title.
    pub text: String,
}

impl SearchNote {
    fn new(title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            text: text.into(),
        }
    }

    /// The note of a text with no term that can run.
    fn needs_a_word() -> Self {
        Self::new(
            "Music Index needs a word or a field to search for",
            "Add a word, a phrase or a field.",
        )
    }
}

/// The note of `query` and its `plan`, or `None` when the Index part has no note
/// (musicindex ADR 0052 R-QUERY-8).
///
/// The first field error in text order comes first. With [`Library::Present`], a
/// query for the Library only has no note, because the Library rows show. A plan
/// that sends a request has a note only when no term of the query can run.
#[must_use]
pub fn search_note(
    query: &SearchQuery,
    plan: &IndexSearchPlan,
    library: Library,
) -> Option<SearchNote> {
    if let Some((_, error)) = query.field_errors().first() {
        return Some(SearchNote::new("This search cannot run", error.message()));
    }
    let skip = match plan {
        IndexSearchPlan::Send(_) => {
            return (!query.has_runnable_content()).then(SearchNote::needs_a_word);
        }
        IndexSearchPlan::Skip(skip) => skip,
    };
    match skip {
        IndexSkip::LibraryOnly => match library {
            Library::Present => None,
            Library::Absent => Some(SearchNote::new(
                "This search uses a Library field",
                "Remove is:, playlist:, has: or in:library to search Music Index.",
            )),
        },
        IndexSkip::Conflict => Some(SearchNote::new(
            "No results",
            "The search asks for the Library and for Music Index only. Remove one of the fields.",
        )),
        IndexSkip::Unsupported(terms) => Some(SearchNote::new(
            terms.note_title(),
            "Remove the - from the field.",
        )),
        // A term that can run, with no request: the `kind:` values send none.
        IndexSkip::NoIndexPart if query.has_runnable_content() => Some(SearchNote::new(
            "No results",
            "Music Index has no rows for these fields.",
        )),
        IndexSkip::ExclusionsOnly | IndexSkip::NoIndexPart => Some(SearchNote::needs_a_word()),
    }
}

/// The note of a scope that gets no request, or `None` (musicindex ADR 0052
/// R-QUERY-8).
///
/// The note names each field that `entity` does not take. It shows only when the
/// plan sends a request and `entity` gets none, so an app gives [`search_note`]
/// first. A track takes each filter, so it never gets this note.
#[must_use]
pub fn scope_note(
    query: &SearchQuery,
    plan: &IndexSearchPlan,
    entity: IndexEntity,
) -> Option<SearchNote> {
    let IndexSearchPlan::Send(request) = plan else {
        return None;
    };
    if request.for_entity(entity).is_some() {
        return None;
    }
    let tokens = refused_tokens(query, entity);
    let (verb, field) = if tokens.len() == 1 {
        ("does", "field")
    } else {
        ("do", "fields")
    };
    let scope = match entity {
        IndexEntity::Publisher => "artists and labels",
        IndexEntity::Feed => "releases",
        IndexEntity::Track => return None,
    };
    Some(SearchNote::new(
        format!("{} {verb} not apply to {scope}", token_list(&tokens)),
        format!("Remove the {field} to search {scope}."),
    ))
}
