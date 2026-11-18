//! The build values of a Rust app of the suite: the version, the git revision
//! and the build time.
//!
//! The request is `docs/plans/research-build-info-request.md`. It extends the
//! pattern of stophammer `deploy.sh`, `build.rs` and `GET /node/info`.
//!
//! An app uses this crate in two places:
//!
//! 1. The build script of the app calls [`build::emit`]. It gives the git
//!    revision and the build time to the compiler of the app.
//! 2. The code of the app calls [`build_info!`]. The macro reads the values
//!    when the app compiles, and gives a [`BuildInfo`].
//!
//! A macro is necessary, because `env!` and `option_env!` read a value when
//! the crate that holds them compiles. A function of this crate reads the
//! values of this crate, not the values of the app.

pub mod build;

use serde::Serialize;

/// The name of the compiler variable that holds the git revision of the app.
pub const GIT_REVISION_VAR: &str = "MUSICINDEX_BUILD_GIT_REVISION";
/// The name of the compiler variable that holds the build time of the app.
pub const BUILT_AT_VAR: &str = "MUSICINDEX_BUILD_BUILT_AT";

/// The build values of one app.
///
/// The JSON object of the info route has the fields `version`,
/// `git_revision` and `built_at`. An unknown value is null.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct BuildInfo {
    /// The name of the app in the `--version` line. It is not in the JSON
    /// object.
    #[serde(skip)]
    pub name: &'static str,
    /// The semantic version of the `Cargo.toml` of the app.
    pub version: &'static str,
    /// The short hash of the commit, with `-dirty` when the build had
    /// uncommitted changes.
    pub git_revision: Option<&'static str>,
    /// The build time in UTC, in the form `2026-11-17T12:00:00Z`.
    pub built_at: Option<&'static str>,
}

impl BuildInfo {
    /// The build values from their texts. An unset value and an empty value
    /// are unknown.
    #[must_use]
    pub const fn new(
        name: &'static str,
        version: &'static str,
        git_revision: Option<&'static str>,
        built_at: Option<&'static str>,
    ) -> Self {
        Self {
            name,
            version,
            git_revision: known(git_revision),
            built_at: known(built_at),
        }
    }

    /// The line for `--version`, for example
    /// `HitIt 0.3.0 (a1b2c3d, 2026-11-17)`.
    ///
    /// The parentheses hold the known values: the git revision and the date
    /// of the build time. When the two values are unknown, the line has no
    /// parentheses.
    #[must_use]
    pub fn version_line(&self) -> String {
        let date = self.built_at.map(|at| at.get(..10).unwrap_or(at));
        let known: Vec<&str> = [self.git_revision, date].into_iter().flatten().collect();
        if known.is_empty() {
            format!("{} {}", self.name, self.version)
        } else {
            format!("{} {} ({})", self.name, self.version, known.join(", "))
        }
    }
}

/// `None` for an unset value and for an empty value.
const fn known(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// The [`BuildInfo`] of the crate that calls the macro.
///
/// `build_info!()` uses the package name of the app. `build_info!("HitIt")`
/// uses the name that the app gives.
#[macro_export]
macro_rules! build_info {
    () => {
        $crate::build_info!(env!("CARGO_PKG_NAME"))
    };
    ($name:expr) => {
        $crate::BuildInfo::new(
            $name,
            env!("CARGO_PKG_VERSION"),
            option_env!("MUSICINDEX_BUILD_GIT_REVISION"),
            option_env!("MUSICINDEX_BUILD_BUILT_AT"),
        )
    };
}
