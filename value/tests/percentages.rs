//! The percentages of the shares method of musicindex ADR 0075.

use musicindex_value::{parse_share, percentages};

const ADR: &str = "musicindex ADR 0075: the percentage of a share is n / S × 100";

/// Compare each percentage with `expected` to 1e-9.
fn assert_near(shares: &[Option<f64>], expected: &[Option<f64>], why: &str) {
    let got = percentages(shares);
    assert_eq!(got.len(), expected.len(), "{why}: {got:?}");
    for (g, e) in got.iter().zip(expected) {
        match (g, e) {
            (Some(g), Some(e)) => assert!((g - e).abs() < 1e-9, "{why}: {got:?}"),
            (None, None) => {}
            _ => panic!("{why}: {got:?}, expected {expected:?}"),
        }
    }
}

#[test]
fn a_fee_of_10_with_two_shares_of_100_is_in_the_total() {
    assert_near(
        &[Some(100.0), Some(100.0), Some(10.0)],
        &[Some(1000.0 / 21.0), Some(1000.0 / 21.0), Some(100.0 / 21.0)],
        ADR,
    );
    let got = percentages(&[Some(100.0), Some(100.0), Some(10.0)]);
    assert_eq!(format!("{:.3}", got[0].unwrap()), "47.619");
    assert_eq!(format!("{:.3}", got[2].unwrap()), "4.762");
}

#[test]
fn one_recipient_gets_100_whatever_its_share() {
    for share in [Some(10.0), None, Some(f64::NAN), Some(-1.0), Some(0.0)] {
        assert_eq!(
            percentages(&[share]),
            [Some(100.0)],
            "musicindex ADR 0075: one recipient gets 100 % (value.md line 172), share {share:?}"
        );
    }
}

#[test]
fn shares_of_100_and_150_give_40_and_60() {
    assert_near(&[Some(100.0), Some(150.0)], &[Some(40.0), Some(60.0)], ADR);
}

#[test]
fn a_share_that_is_not_valid_gives_no_percentage_and_counts_as_0() {
    for bad in [
        None,
        Some(f64::NAN),
        Some(-1.0),
        Some(f64::INFINITY),
        Some(f64::NEG_INFINITY),
    ] {
        assert_near(
            &[Some(1.0), Some(3.0), bad],
            &[Some(25.0), Some(75.0), None],
            "musicindex ADR 0075: a share that is not valid counts as 0",
        );
    }
}

#[test]
fn a_total_of_0_gives_no_percentage() {
    assert_eq!(
        percentages(&[Some(0.0), Some(0.0), None]),
        [None, None, None],
        "musicindex ADR 0075: when S is 0, each item is None"
    );
    assert_eq!(percentages(&[]), Vec::<Option<f64>>::new());
}

#[test]
fn the_example_of_the_specification_49_46_5_and_a_fee_of_1() {
    assert_near(
        &[Some(49.0), Some(46.0), Some(5.0), Some(1.0)],
        &[
            Some(4900.0 / 101.0),
            Some(4600.0 / 101.0),
            Some(500.0 / 101.0),
            Some(100.0 / 101.0),
        ],
        ADR,
    );
    let got: Vec<String> = percentages(&[Some(49.0), Some(46.0), Some(5.0), Some(1.0)])
        .into_iter()
        .map(|p| format!("{:.2}", p.unwrap()))
        .collect();
    assert_eq!(got, ["48.51", "45.54", "4.95", "0.99"]);
}

#[test]
fn the_text_of_a_split_gives_a_share_as_studio_reads_it() {
    assert_eq!(parse_share("12.5"), Some(12.5));
    assert_eq!(parse_share(" 7 "), Some(7.0));
    assert_eq!(parse_share("   "), Some(0.0));
    assert_eq!(parse_share(""), None);
    assert_eq!(parse_share("x"), None);
}
