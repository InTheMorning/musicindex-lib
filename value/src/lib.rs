//! The split of a `podcast:value` block (musicindex ADR 0075).
//!
//! This crate gives the percentage of each recipient of one value block with
//! the shares method of ADR 0075. A fee recipient has the same arithmetic as
//! each other recipient, so the crate does not read the fee flag. The crate
//! has no I/O and no type of an app (musicindex ADR 0074 R-VALUE-4 item 3).

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

/// The percentage of each recipient of one value block (musicindex ADR 0075).
///
/// `shares` has the share of each recipient, fees included. The result has one
/// item for each share, in the same sequence.
///
/// - A valid share is finite and 0 or more. S is the total of the valid shares.
/// - Each valid share n gives n / S × 100.
/// - A share that is not valid gives `None`, and counts as 0 in S.
/// - When S is 0, each item is `None`.
/// - A block with one recipient gives `Some(100.0)` to it, whatever its share
///   (`podcast:value` specification, `value.md` line 172).
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
