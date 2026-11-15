//! The vectors of musicindex ADR 0076 R-ADDRESS-2 item 5.

use musicindex_value::short_address;
use serde_json::Value;

const VECTORS: &str = include_str!("short_address.json");

#[test]
fn r_address_2_item_5_each_vector_gives_its_expected_text() {
    let vectors: Vec<Value> = serde_json::from_str(VECTORS).expect("the vectors file is JSON");
    assert!(!vectors.is_empty(), "the vectors file has vectors");
    for vector in &vectors {
        let input = vector["input"]
            .as_str()
            .expect("each vector has an input text");
        let expected = vector["expected"]
            .as_str()
            .expect("each vector has an expected text");
        assert_eq!(
            short_address(input),
            expected,
            "musicindex ADR 0076 R-ADDRESS-2: the short form of {input:?}. Change \
             short_address or value/tests/short_address.json, and copy the file to each app."
        );
    }
}
