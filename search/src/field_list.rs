//! The rows of the field list for a text and a cursor (musicindex ADR 0052
//! R-QUERY-7).
//!
//! The crate gives the rows, and the app gives the list, the keys and the names.
//! A name row needs a request or the Library of an app, so this module gives only
//! the field and the typed prefix of a name. Each range is a byte range on a
//! character boundary of the text. The JavaScript module gives the same rows with
//! UTF-16 ranges.
//!
//! The module has no I/O.

use std::ops::Range;

use crate::query::{
    CursorPartial, FieldValueState, FixedValueInfo, QueryFieldInfo, QueryFieldName, SearchQuery,
    TermKind, cursor_term, fields_with_prefix, unquote_field_value_prefix,
};

/// Whether the app has a Library (musicindex ADR 0052 R-QUERY-7 item 3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Library {
    /// The app has a Library, and the list shows the Library rows.
    Present,
    /// The app has no Library. The list shows no row of `is:`, `playlist:`,
    /// `has:` or `in:`. A typed Library field gives the note of musicindex ADR
    /// 0049 R-QUERY-2 and no row.
    Absent,
}

impl Library {
    /// `true` when the list shows the rows of `field` in this app. With no
    /// Library, `in:` has no row, because `in:index` alone changes nothing.
    fn shows(self, field: QueryFieldName) -> bool {
        match self {
            Self::Present => true,
            Self::Absent => !field.is_library_only() && field != QueryFieldName::In,
        }
    }
}

/// One row of the field list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldListRow {
    /// A field, with its values and its example.
    Field(&'static QueryFieldInfo),
    /// A fixed value of a field, with its text.
    Value {
        field: QueryFieldName,
        value: &'static FixedValueInfo,
    },
}

/// The field and the typed prefix of the name rows. The prefix has its quotes and
/// its escapes read. The app gives the names from its Library or from the Index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameRequest {
    /// `artist:`, `release:` or `playlist:`.
    pub field: QueryFieldName,
    /// The typed text of the value, maybe empty.
    pub prefix: String,
}

/// The field list for one text and one cursor.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FieldListRows {
    /// The byte range that a row replaces: the typed field name, or the typed
    /// value item with its quotes. `None` when the cursor is in a phrase or in a
    /// word whose colon name is not a field.
    pub edit: Option<Range<usize>>,
    /// The field rows or the fixed value rows, in the order of the field table.
    pub rows: Vec<FieldListRow>,
    /// The name rows that the app adds after `rows`.
    pub names: Option<NameRequest>,
    /// The message of the field under the cursor, when its value is not correct.
    pub error: Option<String>,
}

/// The fields whose value rows are names.
const NAME_FIELDS: [QueryFieldName; 3] = [
    QueryFieldName::Artist,
    QueryFieldName::Release,
    QueryFieldName::Playlist,
];

/// The rows of the field list for `text` and the byte offset `cursor`
/// (musicindex ADR 0052 R-QUERY-7 item 2).
///
/// An offset that is not on a character boundary moves back to the boundary. In
/// a term with no colon, the rows are the fields whose token starts with the
/// term. After the colon of a field, the rows are the fixed values that start
/// with the typed value. A Library field and `in:` give no row when `library` is
/// [`Library::Absent`].
#[must_use]
pub fn field_list_rows(text: &str, cursor: usize, library: Library) -> FieldListRows {
    let term = cursor_term(text, cursor);
    let mut cursor = cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let error = error_under_cursor(text, cursor, library);
    match term.partial {
        CursorPartial::FieldName { prefix } => {
            let start = term.range.start;
            FieldListRows {
                edit: Some(start..cursor.max(start)),
                rows: fields_with_prefix(&prefix)
                    .into_iter()
                    .filter(|info| library.shows(info.name))
                    .map(FieldListRow::Field)
                    .collect(),
                names: None,
                error,
            }
        }
        CursorPartial::FieldValue {
            field,
            prefix,
            value_range,
        } => {
            let shown = library.shows(field);
            let typed = unquote_field_value_prefix(&prefix);
            let rows = if shown {
                field
                    .values_with_prefix(&typed)
                    .into_iter()
                    .map(|value| FieldListRow::Value { field, value })
                    .collect()
            } else {
                Vec::new()
            };
            let names = (shown && NAME_FIELDS.contains(&field)).then_some(NameRequest {
                field,
                prefix: typed,
            });
            FieldListRows {
                edit: Some(value_range),
                rows,
                names,
                error,
            }
        }
        CursorPartial::Other => FieldListRows {
            error,
            ..FieldListRows::default()
        },
    }
}

/// The message of the field term that holds the cursor or ends at it, when its
/// value is not correct and the app shows that field.
fn error_under_cursor(text: &str, cursor: usize, library: Library) -> Option<String> {
    SearchQuery::parse(text).terms().iter().find_map(|term| {
        let under_cursor = term.span.start <= cursor && cursor <= term.span.end;
        match &term.kind {
            TermKind::Field(field) if under_cursor && library.shows(field.name) => {
                match &field.value {
                    FieldValueState::Invalid(error) => Some(error.message()),
                    FieldValueState::Valid(_) | FieldValueState::Incomplete => None,
                }
            }
            _ => None,
        }
    })
}
