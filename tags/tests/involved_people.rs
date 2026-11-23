//! The edit value of a `TIPL` or `TMCL` frame (musicindex ADR 0093 R-TAG-1
//! item 4): the crate owns the text, and a pair that does not read back the
//! same is an error.

use std::path::{Path, PathBuf};

use anyhow::Result;
use id3::{Content, Tag, TagLike};
use musicindex_tags::{
    BadInvolvedPerson, Id3v24Edit, InvolvedPersonCause, NoReferences, involved_people_value,
    read_audio_tags, write_id3v24_edits,
};
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn copy_fixture(dir: &TempDir, name: &str) -> PathBuf {
    let copy = dir.path().join(name);
    std::fs::copy(fixture(name), &copy).expect("copy fixture");
    copy
}

/// The pairs of the HitIt file that the " · " join broke.
const BAND: [(&str, &str); 6] = [
    ("bass", "Josh Moroz"),
    ("drums", "Alex Nicol"),
    ("guitar", "Anthony Columbro"),
    ("keyboards", "Anthony Columbro"),
    ("vocals", "Josh Moroz"),
    ("vocals", "Alex Nicol"),
];

#[test]
fn a_tmcl_with_two_people_on_one_role_reads_back_the_same() -> Result<()> {
    let dir = TempDir::new()?;
    let path = copy_fixture(&dir, "old-frames.mp3");
    let value = involved_people_value(BAND)?;
    assert_eq!(
        value,
        "bass: Josh Moroz / drums: Alex Nicol / guitar: Anthony Columbro / \
         keyboards: Anthony Columbro / vocals: Josh Moroz / vocals: Alex Nicol"
    );

    let edits = [Id3v24Edit {
        frame_label: "TMCL".into(),
        value: value.clone(),
    }];
    write_id3v24_edits(&path, &edits, &NoReferences)?;

    let tag = Tag::read_from_path(&path)?;
    let Some(Content::InvolvedPeopleList(list)) = tag.get("TMCL").map(id3::Frame::content) else {
        panic!("the write gives no TMCL frame");
    };
    let pairs: Vec<(&str, &str)> = list
        .items
        .iter()
        .map(|item| (item.involvement.as_str(), item.involvee.as_str()))
        .collect();
    assert_eq!(pairs, BAND);

    let read = read_audio_tags(&path)?;
    let field = read
        .fields
        .iter()
        .find(|field| field.frame_id == "TMCL")
        .expect("the read gives the TMCL field");
    assert_eq!(field.value, value);
    Ok(())
}

fn cause(pairs: &[(&str, &str)]) -> InvolvedPersonCause {
    let error: BadInvolvedPerson =
        involved_people_value(pairs.iter().copied()).expect_err("the pairs are bad");
    error.cause
}

#[test]
fn an_empty_role_or_name_is_an_error() {
    assert_eq!(
        cause(&[("bass", "Josh"), (" ", "Alex")]),
        InvolvedPersonCause::EmptyRole
    );
    assert_eq!(cause(&[("bass", "")]), InvolvedPersonCause::EmptyName);
}

#[test]
fn a_role_or_name_with_the_pair_separator_is_an_error() {
    assert_eq!(
        cause(&[("bass / drums", "Josh")]),
        InvolvedPersonCause::ContainsPairSeparator
    );
    assert_eq!(
        cause(&[("vocals", "Josh / Alex")]),
        InvolvedPersonCause::ContainsPairSeparator
    );
}

#[test]
fn a_role_with_the_role_separator_is_an_error() {
    assert_eq!(
        cause(&[("lead: vocals", "Josh")]),
        InvolvedPersonCause::RoleContainsRoleSeparator
    );
}

#[test]
fn a_pair_that_reads_back_different_is_an_error() {
    let error = involved_people_value([("bass", "Josh /"), ("drums", "Alex")])
        .expect_err("the name ends with the start of the pair separator");
    assert_eq!(error.cause, InvolvedPersonCause::NoSameReadBack);
    assert_eq!(error.index, 0);
}

#[test]
fn a_name_with_the_role_separator_reads_back_the_same() -> Result<()> {
    assert_eq!(
        involved_people_value([("vocals", "Josh: Jr")])?,
        "vocals: Josh: Jr"
    );
    Ok(())
}
