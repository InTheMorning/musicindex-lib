//! The query text of the Music Index apps (musicindex ADR 0049).
//!
//! This crate reads one query text and gives the Index requests for v4vmm and
//! Studio. It has no I/O, no HTTP type and no type of an app. Each app sends the
//! request with its own HTTP code.

pub mod field_list;
pub mod kind;
pub mod note;
pub mod plan;
pub mod query;
pub mod request;
