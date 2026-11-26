# Research Request: One Crate Gives The Build Values Of Each App

## Status

Complete. `build-info/` holds the crate. The operator accepted musicindex ADR 0083 on
2026-11-18, and ADR 0083 R-VERSION-8 owns the rule. This request came from the orchestrator
session in `~/build/musicindex-research`.

This document binds nothing. ADR 0083 answers
`../musicindex/docs/plans/research-app-version-request.md`. That request gives
the facts and the operator choice of 2026-11-18.

## Request

Add one folder with a crate that gives three build values to a Rust app:

1. The semantic version of the `Cargo.toml` of the app.
2. The git revision, the short hash of the commit, with `-dirty` when the build
   had uncommitted changes.
3. The build time in UTC, in the form `2026-11-17T12:00:00Z`.

The crate also gives the line for `--version`, for example
`HitIt 0.3.0 (a1b2c3d, 2026-11-17)`. It also gives the JSON object of the info
route with the fields `version`, `git_revision` and `built_at`. An unknown value
is null.

## Base Components

Extend the stophammer pattern. Do not make a new design.

- `../stophammer/deploy.sh:212` and `:213` calculate the revision and the time.
- `../stophammer/build.rs` tells Cargo to build again when the two values
  change.
- `../stophammer/src/api.rs:4763` reads the values with `option_env!` and gives
  null for an empty value.

The crate must also work in a Docker build with no `.git` folder. The
stophammer `Dockerfile:33` to `:36` gives the values as build arguments. A
value from the environment comes before a value from Git.

## Users

stophammer, musicindex-studio, HitIt, musicindex-live-relay and
musicindex-live-publisher. The relay and the publisher are not in the app list
of `AGENTS.md`. This crate is the first code here for them.

## Checks

- A test proves the `--version` line and the JSON object for a known revision
  and time.
- A test proves null for an unset value and for an empty value.
- The gate of this repository passes.

Wait for the musicindex ADR before an app pins the crate. If the ADR changes a
field name, the crate follows the ADR.
