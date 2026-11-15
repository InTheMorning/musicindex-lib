//! The percentages of musicindex ADR 0074 R-VALUE-3 items 2 to 7.

use musicindex_value::{Recipient, parse_share, percentages};

fn plain(share: f64) -> Recipient {
    Recipient::new(Some(share), false)
}

fn fee(share: f64) -> Recipient {
    Recipient::new(Some(share), true)
}

#[test]
fn r_value_3_item_7_two_shares_of_100_and_a_fee_of_10() {
    assert_eq!(
        percentages(&[plain(100.0), plain(100.0), fee(10.0)]),
        [Some(45.0), Some(45.0), Some(10.0)],
        "musicindex ADR 0074 R-VALUE-3 item 7"
    );
}

#[test]
fn r_value_3_item_2_a_fee_keeps_its_share() {
    assert_eq!(
        percentages(&[plain(1.0), fee(3.0), fee(2.5)]),
        [Some(94.5), Some(3.0), Some(2.5)],
        "musicindex ADR 0074 R-VALUE-3 item 2"
    );
}

#[test]
fn r_value_3_item_3_the_others_share_the_rest() {
    // The Studio test of ADR 0156 item 4: T = 4 and F = 20.
    assert_eq!(
        percentages(&[
            plain(3.0),
            plain(1.0),
            fee(20.0),
            Recipient::new(parse_share("x"), false),
            Recipient::new(None, false),
        ]),
        [Some(60.0), Some(20.0), Some(20.0), None, None],
        "musicindex ADR 0074 R-VALUE-3 item 3"
    );
}

#[test]
fn r_value_3_item_3_with_no_fee_the_others_share_100() {
    assert_eq!(
        percentages(&[plain(50.0), plain(30.0), plain(20.0)]),
        [Some(50.0), Some(30.0), Some(20.0)],
        "musicindex ADR 0074 R-VALUE-3 item 3"
    );
}

#[test]
fn r_value_3_item_4_a_fee_total_above_100_gives_0_to_the_others() {
    assert_eq!(
        percentages(&[plain(5.0), plain(5.0), fee(80.0), fee(30.0)]),
        [Some(0.0), Some(0.0), Some(80.0), Some(30.0)],
        "musicindex ADR 0074 R-VALUE-3 item 4"
    );
    assert_eq!(
        percentages(&[plain(5.0), fee(150.0)]),
        [Some(0.0), Some(150.0)],
        "musicindex ADR 0074 R-VALUE-3 item 4"
    );
}

#[test]
fn r_value_3_item_5_a_share_that_is_not_valid_gives_no_percentage_and_counts_as_0() {
    for bad in [
        None,
        Some(f64::NAN),
        Some(-1.0),
        Some(f64::INFINITY),
        Some(f64::NEG_INFINITY),
    ] {
        assert_eq!(
            percentages(&[plain(1.0), plain(3.0), Recipient::new(bad, false)]),
            [Some(25.0), Some(75.0), None],
            "musicindex ADR 0074 R-VALUE-3 item 5: the share {bad:?} as a recipient that is not a fee"
        );
        assert_eq!(
            percentages(&[plain(1.0), plain(3.0), Recipient::new(bad, true)]),
            [Some(25.0), Some(75.0), None],
            "musicindex ADR 0074 R-VALUE-3 item 5: the share {bad:?} as a fee"
        );
    }
}

#[test]
fn r_value_3_item_6_a_total_of_0_gives_no_percentage_to_the_others() {
    assert_eq!(
        percentages(&[plain(0.0), Recipient::new(None, false), fee(10.0)]),
        [None, None, Some(10.0)],
        "musicindex ADR 0074 R-VALUE-3 item 6"
    );
    assert_eq!(
        percentages(&[fee(10.0), fee(5.0)]),
        [Some(10.0), Some(5.0)],
        "musicindex ADR 0074 R-VALUE-3 item 6: a block with only fees"
    );
    assert_eq!(percentages(&[]), Vec::<Option<f64>>::new());
}

#[test]
fn the_text_of_a_split_gives_a_share_as_studio_reads_it() {
    assert_eq!(parse_share("12.5"), Some(12.5));
    assert_eq!(parse_share(" 7 "), Some(7.0));
    assert_eq!(parse_share("   "), Some(0.0));
    assert_eq!(parse_share(""), None);
    assert_eq!(parse_share("x"), None);
}
