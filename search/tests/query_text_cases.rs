//! Runs the case file against the crate (musicindex ADR 0049 R-QUERY-3).
//!
//! The JavaScript module runs the same file. A case holds the raw text of a term
//! and no offset, because the two languages have different offsets.

use std::fs;
use std::path::Path;

use musicindex_search::kind::ReleaseKind;
use musicindex_search::plan::{IndexEntity, IndexSearchPlan, IndexSearchRequest, IndexSkip};
use musicindex_search::query::{
    ContentValue, FieldValue, FieldValueError, FieldValueState, LibraryStateValue, QueryScope,
    QueryTerm, ScopeValue, SearchQuery, TermKind,
};
use musicindex_search::request::{IndexOrder, IndexRun, index_filter_pairs, index_order_pairs};
use serde_json::{Value, json};

fn case_file() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases/query-text-cases.json");
    let text = fs::read_to_string(path).expect("the case file reads");
    serde_json::from_str(&text).expect("the case file is JSON")
}

fn value_json(value: &FieldValue) -> Value {
    match value {
        FieldValue::Text(text) => json!({ "text": text }),
        FieldValue::Kinds(kinds) => {
            json!({ "kinds": kinds.iter().map(|kind| kind.word()).collect::<Vec<_>>() })
        }
        FieldValue::Scope(scope) => json!({ "scope": match scope {
            ScopeValue::Library => "library",
            ScopeValue::Index => "index",
        }}),
        FieldValue::Date(date) => json!({
            "first_day": date.first_day.format("%Y-%m-%d").to_string(),
            "last_day": date.last_day.format("%Y-%m-%d").to_string(),
        }),
        FieldValue::Duration(range) => {
            json!({ "min_secs": range.min_secs, "max_secs": range.max_secs })
        }
        FieldValue::LibraryState(state) => json!({ "state": match state {
            LibraryStateValue::Downloaded => "downloaded",
            LibraryStateValue::Partial => "partial",
            LibraryStateValue::Ready => "ready",
            LibraryStateValue::Removed => "removed",
        }}),
        FieldValue::Content(content) => json!({ "content": match content {
            ContentValue::Split => "split",
            ContentValue::Image => "image",
        }}),
    }
}

fn error_code(error: &FieldValueError) -> &'static str {
    match error {
        FieldValueError::UnknownValue { .. } => "unknown_value",
        FieldValueError::OneValueOnly { .. } => "one_value_only",
        FieldValueError::NotADate { .. } => "not_a_date",
        FieldValueError::NotADuration => "not_a_duration",
        FieldValueError::ExcludedScope => "excluded_scope",
    }
}

fn term_json(text: &str, term: &QueryTerm) -> Value {
    let raw = &text[term.span.clone()];
    match &term.kind {
        TermKind::Word(word) => {
            json!({ "raw": raw, "excluded": term.excluded, "kind": "word", "text": word })
        }
        TermKind::Phrase(phrase) => {
            json!({ "raw": raw, "excluded": term.excluded, "kind": "phrase", "text": phrase })
        }
        TermKind::Field(field) => {
            let (state, value, error) = match &field.value {
                FieldValueState::Valid(value) => ("valid", value_json(value), Value::Null),
                FieldValueState::Incomplete => ("incomplete", Value::Null, Value::Null),
                FieldValueState::Invalid(error) => (
                    "invalid",
                    Value::Null,
                    json!({ "code": error_code(error), "message": error.message() }),
                ),
            };
            json!({
                "raw": raw,
                "excluded": term.excluded,
                "kind": "field",
                "field": field.name.info().token,
                "state": state,
                "value": value,
                "error": error,
            })
        }
    }
}

fn scope_word(scope: QueryScope) -> &'static str {
    match scope {
        QueryScope::Everything => "everything",
        QueryScope::LibraryOnly => "library_only",
        QueryScope::IndexOnly => "index_only",
        QueryScope::Conflict => "conflict",
    }
}

fn entity_json(request: &IndexSearchRequest, entity: IndexEntity) -> Value {
    let Some(request) = request.for_entity(entity) else {
        return Value::Null;
    };
    let params: Vec<Value> = index_filter_pairs(&request.filters)
        .into_iter()
        .map(|(name, value)| json!([name, value]))
        .collect();
    json!({ "q": request.q, "params": params })
}

fn plan_json(query: &SearchQuery) -> Value {
    match IndexSearchPlan::for_query(query) {
        IndexSearchPlan::Send(request) => json!({
            "feed": entity_json(&request, IndexEntity::Feed),
            "track": entity_json(&request, IndexEntity::Track),
            "publisher": entity_json(&request, IndexEntity::Publisher),
        }),
        IndexSearchPlan::Skip(IndexSkip::Conflict) => json!({ "skip": "conflict" }),
        IndexSearchPlan::Skip(IndexSkip::LibraryOnly) => json!({ "skip": "library_only" }),
        IndexSearchPlan::Skip(IndexSkip::NoIndexPart) => json!({ "skip": "no_index_part" }),
        IndexSearchPlan::Skip(IndexSkip::ExclusionsOnly) => json!({ "skip": "exclusions_only" }),
        IndexSearchPlan::Skip(IndexSkip::Unsupported(terms)) => {
            json!({ "skip": "unsupported", "tokens": terms.field_tokens })
        }
    }
}

fn case_json(text: &str) -> Value {
    let query = SearchQuery::parse(text);
    json!({
        "text": text,
        "terms": query.terms().iter().map(|term| term_json(text, term)).collect::<Vec<_>>(),
        "scope": scope_word(query.scope()),
        "index_q": query.index_q(),
        "runnable": query.has_runnable_content(),
        "plan": plan_json(&query),
    })
}

#[test]
fn adr_0049_r_query_3_the_crate_gives_each_result_of_the_case_file() {
    let file = case_file();
    let cases = file["cases"].as_array().expect("the file has cases");
    assert!(!cases.is_empty());
    let mut differences = Vec::new();
    for case in cases {
        let text = case["text"].as_str().expect("a case has a text");
        let actual = case_json(text);
        if &actual != case {
            differences.push(format!("{text:?}\n  case:  {case}\n  crate: {actual}"));
        }
    }
    assert!(
        differences.is_empty(),
        "musicindex ADR 0049 R-QUERY-3: {} of {} cases differ. Correct the crate, or change the \
         case with the ADR that owns the rule.\n{}",
        differences.len(),
        cases.len(),
        differences.join("\n")
    );
}

/// The two languages differ on these texts (musicindex ADR 0049 R-QUERY-3 item 3).
#[test]
fn adr_0049_r_query_3_item_3_the_file_holds_the_language_differences() {
    let file = case_file();
    let cases = file["cases"].as_array().expect("the file has cases");
    for part in [
        "\u{85}",
        "\u{feff}",
        "\u{212a}",
        "after:2026-02-30",
        "after:0000-01-01",
    ] {
        assert!(
            cases
                .iter()
                .any(|case| case["text"].as_str().is_some_and(|t| t.contains(part))),
            "musicindex ADR 0049 R-QUERY-3 item 3: add a case with {part:?} to the case file."
        );
    }
}

fn order_of(case: &Value) -> IndexOrder {
    let reverse = case["reverse"].as_bool().expect("a case has a direction");
    match case["sort"].as_str() {
        Some("relevance") => {
            assert!(
                !reverse,
                "Stophammer ADR 0084 section 5: relevance has no reverse"
            );
            IndexOrder::Relevance
        }
        Some("name") => IndexOrder::Name { reverse },
        Some("date") => IndexOrder::Date { reverse },
        other => panic!("a case has no sort: {other:?}"),
    }
}

#[test]
fn adr_0084_the_crate_gives_each_order_of_the_case_file() {
    let file = case_file();
    let cases = file["order_cases"]
        .as_array()
        .expect("the file has order cases");
    assert!(!cases.is_empty());
    for case in cases {
        let run = match case["run"].as_str() {
            Some("live") => IndexRun::Live,
            Some("command") => IndexRun::Command,
            other => panic!("a case has no run: {other:?}"),
        };
        let capabilities: Vec<&str> = case["capabilities"]
            .as_array()
            .expect("a case has capabilities")
            .iter()
            .map(|name| name.as_str().expect("a capability is text"))
            .collect();
        let actual: Vec<Value> = index_order_pairs(order_of(case), run, &capabilities)
            .into_iter()
            .map(|(name, value)| json!([name, value]))
            .collect();
        assert_eq!(
            &Value::Array(actual),
            &case["params"],
            "Stophammer ADR 0084: {case}. Correct request::index_order_pairs, or change the case \
             with the ADR that owns the rule."
        );
    }
}

#[test]
fn adr_0049_r_query_6_the_crate_gives_each_label_of_the_case_file() {
    let file = case_file();
    let cases = file["label_cases"]
        .as_array()
        .expect("the file has label cases");
    assert!(!cases.is_empty());
    for case in cases {
        let count = case["track_count"]
            .as_u64()
            .expect("a case has a track count");
        let count = usize::try_from(count).expect("a track count fits");
        let label = ReleaseKind::resolve(case["stated"].as_str(), count).label();
        assert_eq!(
            Some(label),
            case["label"].as_str(),
            "musicindex ADR 0020 R-WORD-1: {case}. Correct kind::ReleaseKind::resolve, or change \
             the case with the ADR that owns the rule."
        );
    }
}
