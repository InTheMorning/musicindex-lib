//! The keys of a name: its identity and its place in a sequence.
//!
//! Stophammer ADR 0070 §1 and ADR 0090 own these keys. Stophammer sorts its lists
//! by [`order_key`], and an app that merges its own rows with node rows by name
//! must use the same key. This module holds one copy of the two functions for the
//! apps, so the apps cannot drift from each other.
//!
//! Stophammer keeps its own copy of the two functions. This module is the client
//! side of the sort contract, and `tests/fixtures/order-key-vectors.json` of
//! Stophammer is that contract. The crate holds a copy of that file, and
//! [`check_vectors`] runs it, so a client copy that differs from the node fails.
//!
//! The keys read the Unicode tables of the Rust standard library and of the crate
//! `unicode-normalization`. A different version of either one can give a
//! different key for a character that a new Unicode version adds.
//!
//! The module has no I/O.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// The width of a run of digits in an order key (Stophammer ADR 0090 §2). A
/// longer run does not change.
const ORDER_KEY_DIGITS: usize = 20;

/// The first character of an order key whose text does not start with a letter
/// from `a` to `z` (Stophammer ADR 0090 §2). It comes before `a`.
pub const ORDER_KEY_OTHER_GROUP: char = '#';

/// Gives the identity key of a name (Stophammer ADR 0070 §1, ADR 0090 §1).
///
/// The function trims the text, joins each run of white space into one space,
/// and lowercases the text by the Unicode rule. It keeps a leading article, the
/// punctuation and each accent. Two names with the same key are one name.
#[must_use]
pub fn sort_key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .to_lowercase()
}

/// Gives the key of a sequence of names (Stophammer ADR 0090 §2).
///
/// The function starts with [`sort_key`], then takes three steps:
///
/// 1. It removes each combining mark after NFKD decomposition, and changes the
///    letters that have no decomposition: `ø` to `o`, `æ` to `ae`, `œ` to `oe`,
///    `ß` to `ss`, `ł` to `l`, `đ` to `d`, `þ` to `th`, `ð` to `d` and `ı` to
///    `i`.
/// 2. It puts zeros before each run of ASCII digits, to 20 digits, so that a
///    number sorts by its value.
/// 3. It puts [`ORDER_KEY_OTHER_GROUP`] first when the text does not start with
///    a letter from `a` to `z`. An empty text gives an empty key.
///
/// Two order keys compare by code point. The identity of a name stays
/// [`sort_key`].
///
/// # Examples
///
/// ```
/// use musicindex_search::name_order::order_key;
///
/// assert_eq!(order_key("Øystein"), "oystein");
/// assert!(order_key("track 2") < order_key("track 10"));
/// assert!(order_key("50 Cent") < order_key("ABBA"));
/// ```
#[must_use]
pub fn order_key(text: &str) -> String {
    let mut folded = String::new();
    for ch in sort_key(text).nfkd().filter(|ch| !is_combining_mark(*ch)) {
        match ch {
            'ø' => folded.push('o'),
            'æ' => folded.push_str("ae"),
            'œ' => folded.push_str("oe"),
            'ß' => folded.push_str("ss"),
            'ł' => folded.push('l'),
            'đ' | 'ð' => folded.push('d'),
            'þ' => folded.push_str("th"),
            'ı' => folded.push('i'),
            other => folded.push(other),
        }
    }

    // An empty text has an empty key, so an empty `from` names no position.
    if folded.is_empty() {
        return folded;
    }
    let mut key = String::with_capacity(folded.len() + 1);
    if !folded.starts_with(|ch: char| ch.is_ascii_lowercase()) {
        key.push(ORDER_KEY_OTHER_GROUP);
    }
    let mut digits = String::new();
    for ch in folded.chars().chain(std::iter::once('\0')) {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if !digits.is_empty() {
            for _ in digits.len()..ORDER_KEY_DIGITS {
                key.push('0');
            }
            key.push_str(&digits);
            digits.clear();
        }
        if ch != '\0' {
            key.push(ch);
        }
    }
    key
}

/// The vector file of Stophammer ADR 0090, a copy of
/// `tests/fixtures/order-key-vectors.json` of Stophammer.
const VECTORS: &str = include_str!("../cases/order-key-vectors.json");

/// Checks [`sort_key`] and [`order_key`] against each vector of Stophammer ADR
/// 0090.
///
/// An app calls this function in one of its own tests. The app then runs the
/// vectors with its own toolchain and its own lock file, so a change of a Unicode
/// table fails in the tests of that app.
///
/// # Errors
///
/// Gives each vector whose key differs, with the input and the two keys, or a
/// message when the vector file cannot be read.
pub fn check_vectors() -> Result<(), String> {
    let file: serde_json::Value = serde_json::from_str(VECTORS)
        .map_err(|error| format!("Stophammer ADR 0090: the vector file is not JSON ({error})."))?;
    let vectors = file["vectors"]
        .as_array()
        .ok_or("Stophammer ADR 0090: the vector file has no list `vectors`.")?;
    let mut differences = Vec::new();
    for vector in vectors {
        let (Some(input), Some(sort), Some(order)) = (
            vector["input"].as_str(),
            vector["sort_key"].as_str(),
            vector["order_key"].as_str(),
        ) else {
            return Err(format!(
                "Stophammer ADR 0090: a vector has no `input`, `sort_key` or `order_key`: {vector}"
            ));
        };
        for (name, want, got) in [
            ("sort_key", sort, sort_key(input)),
            ("order_key", order, order_key(input)),
        ] {
            if got != want {
                differences.push(format!(
                    "{name}({input:?}) is {got:?}, and the vector gives {want:?}"
                ));
            }
        }
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Stophammer ADR 0090: {} of {} vectors differ. A Unicode table of the toolchain or of \
             unicode-normalization differs from the node, or the function changed. Compare the \
             versions with Stophammer, or correct name_order.\n{}",
            differences.len(),
            vectors.len(),
            differences.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::check_vectors;

    #[test]
    fn stophammer_adr_0090_each_vector_gives_its_keys() {
        if let Err(message) = check_vectors() {
            panic!("{message}");
        }
    }
}
