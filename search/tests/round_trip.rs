//! Compares what the crate means by `q` with what Stophammer reads from it.
//!
//! This is the round trip test of `tools/query-text` in `musicindex-research`,
//! with the rule of musicindex ADR 0049 R-QUERY-5. `sanitize` is a copy of
//! `sanitize_api_query_value` in HitIt `src/api.rs`. `fts5_phrases` and
//! `push_words` are copies from Stophammer `src/search.rs`.

use std::fs;
use std::path::Path;

use musicindex_search::query::{SearchQuery, TermKind};
use serde_json::Value;

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch == '\0' || ch == '\\' || ch.is_control() {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fts5_phrases(input: &str) -> Vec<String> {
    let cleaned: String = input.chars().filter(|c| !c.is_control()).collect();
    let mut phrases = Vec::new();
    let mut rest = cleaned.as_str();
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            break;
        };
        push_words(&mut phrases, &rest[..open]);
        let words: Vec<&str> = after[..close].split_whitespace().collect();
        if !words.is_empty() {
            phrases.push(format!("\"{}\"", words.join(" ")));
        }
        rest = &after[close + 1..];
    }
    push_words(&mut phrases, rest);
    phrases
}

fn push_words(phrases: &mut Vec<String>, text: &str) {
    phrases.extend(
        text.split_whitespace()
            .map(|word| format!("\"{}\"", word.replace('"', "\"\""))),
    );
}

/// The FTS5 phrases that the typed terms mean. A quote and a backslash are
/// separators, so a word with two parts gives two phrases, and a phrase stays
/// one phrase.
fn intended_phrases(query: &SearchQuery) -> Vec<String> {
    let spaced = |text: &str| text.replace(['"', '\\'], " ");
    let mut phrases = Vec::new();
    for term in query.terms().iter().filter(|term| !term.excluded) {
        match &term.kind {
            TermKind::Word(word) => {
                let cleaned: String = word.chars().filter(|c| !c.is_control()).collect();
                phrases.extend(
                    spaced(&cleaned)
                        .split_whitespace()
                        .map(|part| format!("\"{part}\"")),
                );
            }
            TermKind::Phrase(phrase) => {
                let spaced = spaced(phrase);
                let words: Vec<&str> = spaced.split_whitespace().collect();
                if !words.is_empty() {
                    phrases.push(format!("\"{}\"", words.join(" ")));
                }
            }
            TermKind::Field(_) => {}
        }
    }
    phrases
}

#[test]
fn adr_0049_r_query_5_the_round_trip_gives_no_difference_for_q() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases/query-text-cases.json");
    let file: Value = serde_json::from_str(&fs::read_to_string(path).expect("the case file reads"))
        .expect("the case file is JSON");
    let mut differences = Vec::new();
    for case in file["cases"].as_array().expect("the file has cases") {
        let text = case["text"].as_str().expect("a case has a text");
        let query = SearchQuery::parse(text);
        let Some(q) = query.index_q() else {
            continue;
        };
        let read = fts5_phrases(&sanitize(&q));
        let intended = intended_phrases(&query);
        if read != intended {
            differences.push(format!(
                "{text:?}: q {q:?} read {read:?} intended {intended:?}"
            ));
        }
    }
    assert!(
        differences.is_empty(),
        "musicindex ADR 0049 R-QUERY-5: Stophammer reads other phrases. Write a space for each \
         quote and each backslash in query::index_word and query::index_phrase.\n{}",
        differences.join("\n")
    );
}
