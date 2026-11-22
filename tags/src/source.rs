//! The source of the pictures and the transcripts that an edit names.

use anyhow::{Result, anyhow};

/// Gives the content of a reference in an `APIC`, `USLT` or `SYLT` edit. A
/// reference is a URL or a local path. The app owns the network, so the app
/// implements this trait.
pub trait ReferenceSource {
    /// The text of the transcript at `reference`.
    ///
    /// # Errors
    ///
    /// Returns an error when the text cannot be read, or when the content is
    /// not a transcript, for example an HTML page.
    fn text(&self, reference: &str) -> Result<String>;

    /// The MIME type and the bytes of the picture at `reference`.
    ///
    /// # Errors
    ///
    /// Returns an error when the picture cannot be read, or when the bytes are
    /// not an image.
    fn picture(&self, reference: &str) -> Result<(String, Vec<u8>)>;
}

/// A source with no content. Use it for a write that has no `APIC`, `USLT` or
/// `SYLT` edit. An edit that needs a reference then gives an error.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoReferences;

impl ReferenceSource for NoReferences {
    fn text(&self, reference: &str) -> Result<String> {
        Err(anyhow!(
            "no reference source for the transcript {reference}"
        ))
    }

    fn picture(&self, reference: &str) -> Result<(String, Vec<u8>)> {
        Err(anyhow!("no reference source for the picture {reference}"))
    }
}
