//! The vector file of Stophammer ADR 0090 is a copy (musicindex-lib AGENTS.md).
//!
//! The test compares the copy with the file at the committed HEAD of a Stophammer
//! checkout next to this repository. The CI of this repository has no such
//! checkout, so the test reports a skip there.

use std::fs;
use std::path::Path;
use std::process::Command;

const STOPHAMMER_FILE: &str = "tests/fixtures/order-key-vectors.json";

#[test]
fn stophammer_adr_0090_the_vector_file_is_a_copy() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stophammer = root.join("../../stophammer");
    let output = Command::new("git")
        .arg("-C")
        .arg(&stophammer)
        .args(["show", &format!("HEAD:{STOPHAMMER_FILE}")])
        .output();
    let Some(output) = output.ok().filter(|output| output.status.success()) else {
        eprintln!(
            "skip: no Stophammer checkout with a committed {STOPHAMMER_FILE} at {}",
            stophammer.display()
        );
        return;
    };
    let copy = fs::read(root.join("cases/order-key-vectors.json")).expect("the copy reads");
    assert!(
        output.stdout == copy,
        "Stophammer ADR 0090: search/cases/order-key-vectors.json has other bytes than \
         {STOPHAMMER_FILE} at the HEAD of Stophammer. Copy the file from Stophammer, and name its \
         commit in the commit message."
    );
}
