//! The values of a `podcast:value` block that the suite apps show.
//!
//! - `percentages` gives the percentage of each recipient of one value block
//!   with the shares method of musicindex ADR 0075 R-VALUE-5. A fee recipient
//!   has the same arithmetic as each other recipient, so the crate does not
//!   read the fee flag.
//! - `parse_share` reads the text of a split.
//! - `short_address` gives the short form of a long address (musicindex ADR
//!   0076 R-ADDRESS-2).
//!
//! The crate has no I/O and no type of an app (musicindex ADR 0075 R-VALUE-4
//! item 3).

/// The share in the text of a `split` attribute.
///
/// Empty text gives `None`. Text of only spaces gives 0. Other text gives its
/// number, or `None` when it is not a number. Studio reads a split with this
/// rule (Studio ADR 0133).
#[must_use]
pub fn parse_share(text: &str) -> Option<f64> {
    if text.is_empty() {
        return None;
    }
    let text = text.trim();
    if text.is_empty() {
        return Some(0.0);
    }
    text.parse::<f64>().ok()
}

/// The percentage of each recipient of one value block (musicindex ADR 0075 R-VALUE-5).
///
/// `shares` has the share of each recipient, fees included. The result has one
/// item for each share, in the same sequence.
///
/// - A valid share is finite and 0 or more. S is the total of the valid shares.
/// - Each valid share n gives n / S × 100.
/// - A share that is not valid gives `None`, and counts as 0 in S.
/// - When S is 0, each item is `None`.
/// - A block with one recipient gives `Some(100.0)` to it, whatever its share
///   (`podcast:value` specification, `value.md` lines 173 to 176).
///
/// The app shows a percentage with one decimal place.
#[must_use]
pub fn percentages(shares: &[Option<f64>]) -> Vec<Option<f64>> {
    if shares.len() == 1 {
        return vec![Some(100.0)];
    }
    let valid = |share: Option<f64>| share.filter(|n| n.is_finite() && *n >= 0.0);
    let total: f64 = shares.iter().filter_map(|s| valid(*s)).sum();
    shares
        .iter()
        .map(|s| {
            let n = valid(*s)?;
            (total > 0.0).then(|| n / total * 100.0)
        })
        .collect()
}

/// The number of characters of a text that shows in full.
const FULL_LENGTH: usize = 20;
/// The number of characters before the ellipsis of a short form.
const HEAD: usize = 10;
/// The number of characters after the ellipsis of a short form.
const TAIL: usize = 8;

/// The short form of a node address or a Nostr public key (musicindex ADR 0076
/// R-ADDRESS-2).
///
/// The function counts characters, not bytes. It gives the text unchanged in
/// these conditions:
///
/// - The text has 20 characters or fewer.
/// - The text starts with `lnurl`, in lowercase or uppercase letters.
/// - The text contains "@".
///
/// Otherwise it gives the first 10 characters, "…" (U+2026) and the last 8
/// characters. The app calls this function only for a node address or a Nostr
/// public key.
#[must_use]
pub fn short_address(text: &str) -> String {
    let length = text.chars().count();
    let lnurl = text
        .get(..5)
        .is_some_and(|start| start.eq_ignore_ascii_case("lnurl"));
    if length <= FULL_LENGTH || lnurl || text.contains('@') {
        return text.to_owned();
    }
    let head: String = text.chars().take(HEAD).collect();
    let tail: String = text.chars().skip(length - TAIL).collect();
    format!("{head}\u{2026}{tail}")
}
