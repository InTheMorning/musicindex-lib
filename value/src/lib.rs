//! The split of a `podcast:value` block (musicindex ADR 0074).
//!
//! This crate gives the percentage of each recipient of one value block, as
//! R-VALUE-3 states. It has no I/O and no type of an app (R-VALUE-4 item 3).
//! For each recipient, the app gives the share and the fee flag.

/// One recipient of a value block, as the split needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipient {
    /// The share of the recipient. `None` is a share that is not a number.
    pub share: Option<f64>,
    /// `true` when the recipient is a fee recipient.
    pub fee: bool,
}

impl Recipient {
    /// A recipient with the share `share` and the fee flag `fee`.
    #[must_use]
    pub const fn new(share: Option<f64>, fee: bool) -> Self {
        Self { share, fee }
    }

    /// The share as R-VALUE-3 item 5 uses it. A share that is not a number,
    /// a share below 0 and a share that is not finite give `None`.
    fn valid_share(self) -> Option<f64> {
        self.share.filter(|n| n.is_finite() && *n >= 0.0)
    }
}

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

/// The percentage of each recipient of one value block (R-VALUE-3 items 2 to 6).
///
/// The result has one item for each recipient, in the same sequence.
///
/// - A fee recipient has its share as its percentage.
/// - Each other recipient with the share n has n / T × (100 − F). T is the
///   total of the shares that are not fees. F is the total of the fee shares.
/// - When F is more than 100, each recipient that is not a fee gets 0.
/// - A share that is not valid gives `None`, and counts as 0 in T and in F.
/// - When T is 0, each recipient that is not a fee gets `None`.
///
/// The app shows a percentage with one decimal place (R-VALUE-3 item 1).
#[must_use]
pub fn percentages(recipients: &[Recipient]) -> Vec<Option<f64>> {
    let total = |fee: bool| -> f64 {
        recipients
            .iter()
            .filter(|r| r.fee == fee)
            .map(|r| r.valid_share().unwrap_or(0.0))
            .sum()
    };
    let fees = total(true);
    let others = total(false);
    let rest = (100.0 - fees).max(0.0);
    recipients
        .iter()
        .map(|r| {
            let n = r.valid_share()?;
            if r.fee {
                Some(n)
            } else if others > 0.0 {
                Some(n / others * rest)
            } else {
                None
            }
        })
        .collect()
}
