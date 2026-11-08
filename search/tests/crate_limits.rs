//! The limits of the crate (musicindex ADR 0049 R-QUERY-4 item 2).

use std::fs;
use std::path::Path;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn adr_0049_r_query_4_item_2_the_crate_has_no_http_dependency() {
    let manifest = fs::read_to_string(Path::new(ROOT).join("Cargo.toml")).expect("manifest reads");
    let table = manifest
        .split("[dependencies]")
        .nth(1)
        .and_then(|rest| rest.split("\n[").next())
        .expect("the manifest has dependencies");
    let names: Vec<&str> = table
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.trim()))
        .collect();
    assert_eq!(
        names,
        ["chrono", "serde_json", "unicode-normalization"],
        "musicindex ADR 0049 R-QUERY-4 item 2: the crate has no HTTP dependency. Keep the HTTP \
         code in the app, and give the app the parameters. unicode-normalization is the \
         dependency of Stophammer ADR 0090 §5 for the order key, and serde_json reads \
         its vector file."
    );
}

#[test]
fn adr_0049_r_query_4_item_2_the_source_has_no_io_and_no_app_type() {
    let source = Path::new(ROOT).join("src");
    for entry in fs::read_dir(source).expect("the source folder reads") {
        let path = entry.expect("the entry reads").path();
        let text = fs::read_to_string(&path).expect("the file reads");
        for line in text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
        {
            for name in [
                "std::fs",
                "std::net",
                "reqwest",
                "ureq",
                "gpui",
                "view_models",
            ] {
                assert!(
                    !line.contains(name),
                    "musicindex ADR 0049 R-QUERY-4 item 2: {} names {name}. The crate has no I/O \
                     and no type of an app. Move this code to the app.",
                    path.display()
                );
            }
        }
    }
}
