//! The part of the crate for the build script of an app.
//!
//! The build script of the app calls [`emit`] with the prefix of its input
//! variables. stophammer gives `"STOPHAMMER"`, and then reads
//! `STOPHAMMER_GIT_REVISION` and `STOPHAMMER_BUILT_AT` as `deploy.sh` and the
//! `Dockerfile` set them.
//!
//! A value from the environment comes before a value from Git. A build with
//! no `.git` folder and no input variable has no git revision.

use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{BUILT_AT_VAR, GIT_REVISION_VAR};

/// Give the git revision and the build time to the compiler of the app.
///
/// Call this function from the `build.rs` of the app. The input variables are
/// `{prefix}_GIT_REVISION` and `{prefix}_BUILT_AT`.
///
/// - The git revision is the input variable. When it is unset or empty, it is
///   the short hash of `HEAD`, with `-dirty` when a tracked file has changes.
/// - The build time is the input variable. When it is unset or empty, it is
///   the time of this build script in UTC.
///
/// Cargo runs the build script again when an input variable changes, when
/// `HEAD`, the file of the branch or the Git index changes, and when a file in
/// `src` or `Cargo.toml` changes. The build time is the time of the last run of
/// the build script. A release build sets `{prefix}_BUILT_AT`.
///
/// When the first build has no `.git` folder, Cargo watches no Git file. A
/// `cargo clean` then gives the git revision.
pub fn emit(prefix: &str) {
    let revision_var = format!("{prefix}_GIT_REVISION");
    let built_at_var = format!("{prefix}_BUILT_AT");
    println!("cargo:rerun-if-env-changed={revision_var}");
    println!("cargo:rerun-if-env-changed={built_at_var}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let git_dir = PathBuf::from(git_dir);
        for file in ["HEAD", "index"] {
            println!("cargo:rerun-if-changed={}", git_dir.join(file).display());
        }
        // A commit changes the file of the branch, not `HEAD`.
        if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={}", git_dir.join(branch).display());
        }
    }

    if let Some(revision) = choose(env::var(&revision_var).ok(), git_revision) {
        println!("cargo:rustc-env={GIT_REVISION_VAR}={revision}");
    }
    if let Some(built_at) = choose(env::var(&built_at_var).ok(), now) {
        println!("cargo:rustc-env={BUILT_AT_VAR}={built_at}");
    }
}

/// The value of the environment when it is set and not empty. Otherwise the
/// value of `fallback`.
pub fn choose(
    from_env: Option<String>,
    fallback: impl FnOnce() -> Option<String>,
) -> Option<String> {
    from_env.filter(|v| !v.is_empty()).or_else(fallback)
}

/// The short hash of `HEAD`, with `-dirty` when a tracked file has changes.
/// This is the command of stophammer `deploy.sh` line 212.
fn git_revision() -> Option<String> {
    let hash = git(&["rev-parse", "--short", "HEAD"])?;
    let changes = git(&["status", "--porcelain", "--untracked-files=no"]).unwrap_or_default();
    Some(if changes.is_empty() {
        hash
    } else {
        format!("{hash}-dirty")
    })
}

/// The output of one Git command, with no space at the start and the end.
/// `None` when Git is missing or the command fails.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_owned())
}

/// The time now in UTC.
fn now() -> Option<String> {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(utc_text(seconds))
}

/// A Unix time in seconds as UTC text, in the form `2026-11-17T12:00:00Z`.
///
/// This is the form of `date -u +%Y-%m-%dT%H:%M:%SZ`. The date arithmetic is
/// the algorithm `civil_from_days` of Howard Hinnant.
#[must_use]
pub fn utc_text(seconds: u64) -> String {
    let days = seconds / 86_400;
    let rest = seconds % 86_400;
    let (hour, minute, second) = (rest / 3600, rest % 3600 / 60, rest % 60);

    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}
