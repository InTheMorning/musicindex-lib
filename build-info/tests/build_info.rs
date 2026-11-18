//! The checks of `docs/plans/research-build-info-request.md`.

use musicindex_build_info::BuildInfo;
use musicindex_build_info::build::{choose, utc_text};
use serde_json::json;

const KNOWN: BuildInfo = BuildInfo::new(
    "HitIt",
    "0.3.0",
    Some("a1b2c3d"),
    Some("2026-11-17T12:00:00Z"),
);

#[test]
fn the_version_line_of_a_known_revision_and_time() {
    assert_eq!(KNOWN.version_line(), "HitIt 0.3.0 (a1b2c3d, 2026-11-17)");
    let dirty = BuildInfo::new("HitIt", "0.3.0", Some("a1b2c3d-dirty"), None);
    assert_eq!(dirty.version_line(), "HitIt 0.3.0 (a1b2c3d-dirty)");
    let no_revision = BuildInfo::new("HitIt", "0.3.0", None, Some("2026-11-17T12:00:00Z"));
    assert_eq!(no_revision.version_line(), "HitIt 0.3.0 (2026-11-17)");
}

#[test]
fn the_json_object_of_a_known_revision_and_time() {
    assert_eq!(
        serde_json::to_value(KNOWN).expect("the object serializes"),
        json!({
            "version": "0.3.0",
            "git_revision": "a1b2c3d",
            "built_at": "2026-11-17T12:00:00Z",
        })
    );
}

#[test]
fn an_unset_value_and_an_empty_value_are_null() {
    for value in [None, Some("")] {
        let info = BuildInfo::new("HitIt", "0.3.0", value, value);
        assert_eq!(info.git_revision, None);
        assert_eq!(info.built_at, None);
        assert_eq!(
            serde_json::to_value(info).expect("the object serializes"),
            json!({ "version": "0.3.0", "git_revision": null, "built_at": null })
        );
        assert_eq!(info.version_line(), "HitIt 0.3.0");
    }
}

#[test]
fn the_macro_reads_the_package_of_the_caller() {
    let info = musicindex_build_info::build_info!();
    assert_eq!(info.name, "musicindex-build-info");
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    let named = musicindex_build_info::build_info!("Build Info");
    assert_eq!(named.name, "Build Info");
}

#[test]
fn a_value_from_the_environment_comes_before_a_value_from_git() {
    let git = || Some("fromgit".to_owned());
    assert_eq!(
        choose(Some("fromenv".to_owned()), git),
        Some("fromenv".to_owned())
    );
    assert_eq!(choose(Some(String::new()), git), Some("fromgit".to_owned()));
    assert_eq!(choose(None, git), Some("fromgit".to_owned()));
    assert_eq!(choose(None, || None), None);
}

#[test]
fn the_build_time_is_utc_text() {
    assert_eq!(utc_text(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc_text(1_779_019_200), "2026-05-17T12:00:00Z");
    assert_eq!(utc_text(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(utc_text(1_798_761_599), "2026-12-31T23:59:59Z");
}
