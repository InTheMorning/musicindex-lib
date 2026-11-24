//! The one WAV to FLAC converter of the suite (musicindex ADR 0094).
//!
//! HitIt and musicindex-live-publisher convert through this crate. No app
//! keeps its own copy (R-CONVERT-1).
//!
//! The converter starts external programs. It tries `flac` first, and then
//! `ffmpeg` (R-CONVERT-2). Each program is an optional dependency of an app
//! package (musicindex ADR 0084 R-PACKAGE-4). When no program passes its
//! probe, a function gives [`ConvertError::NoConverter`], and the app decides
//! what to do (R-CONVERT-6).
//!
//! The crate operates only on Unix, because the probe uses a Unix socket.

mod probe;

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use musicindex_tags::format::AudioFormat;

pub use probe::{
    CONVERTER_PROBE_OUTPUT_CAP, CONVERTER_PROBE_TIMEOUT, ConverterObservation, ConverterProbe,
    ExecutableSource, ProbeOutcome,
};

/// `true` when the `flac` program passes its probe. `flac` is the path that
/// the app gives, or `None` for `flac` on `PATH`.
#[must_use]
pub fn flac_cli_available(flac: Option<&Path>) -> bool {
    ConverterProbe::flac(flac).available()
}

/// Converts `wav` to a FLAC file with the same name and the extension
/// `flac`, and deletes `wav` after the FLAC file is good (R-CONVERT-5 item
/// 2). Gives the path of the FLAC file.
///
/// # Errors
///
/// Gives [`ConvertError`] when the output exists, no converter passes its
/// probe, each converter fails, or a file operation fails. Then `wav` stays.
pub fn transcode_wav_to_flac(wav: &Path, flac: Option<&Path>) -> Result<PathBuf, ConvertError> {
    let (path, _) = convert_with(wav, &ConverterObservation::refresh(flac))?;
    std::fs::remove_file(wav).map_err(|source| ConvertError::Io {
        path: wav.to_owned(),
        source,
    })?;
    Ok(path)
}

/// Converts `wav` to a FLAC file with the same name and the extension
/// `flac`, and keeps `wav` (R-CONVERT-5 item 1). The function probes the
/// programs first. Gives the path and the converter that made the file.
///
/// # Errors
///
/// Gives [`ConvertError`] as [`convert_with`] does.
pub fn convert_retaining_input(
    wav: &Path,
    flac: Option<&Path>,
) -> Result<(PathBuf, ConversionOutcome), ConvertError> {
    convert_with(wav, &ConverterObservation::refresh(flac))
}

/// Converts `wav` to a FLAC file with the probes of `observation`, and keeps
/// `wav`. An app that converts many files can probe one time.
///
/// The function uses `flac` when its probe passed. When `flac` failed or did
/// not pass its probe, it uses `ffmpeg` when the probe of `ffmpeg` passed.
/// It removes a failed output file (R-CONVERT-2).
///
/// # Errors
///
/// - [`ConvertError::OutputExists`] when the FLAC path has a file.
/// - [`ConvertError::NoConverter`] when no program passed its probe.
/// - [`ConvertError::Encode`] when each converter that the function tried
///   failed.
/// - [`ConvertError::Io`] when the function cannot remove a failed output.
pub fn convert_with(
    wav: &Path,
    observation: &ConverterObservation,
) -> Result<(PathBuf, ConversionOutcome), ConvertError> {
    let output = wav.with_extension("flac");
    if output.exists() {
        return Err(ConvertError::OutputExists(output));
    }

    let flac_failure = if observation.flac.available() {
        match encode(
            &observation.flac.executable,
            &flac_arguments(wav, &output),
            &output,
        ) {
            Ok(()) => return Ok((output, ConversionOutcome::Flac)),
            Err(failure) => {
                remove_failed_output(&output)?;
                Some(failure)
            }
        }
    } else {
        None
    };

    if !observation.ffmpeg.available() {
        return match flac_failure {
            Some(flac) => Err(ConvertError::Encode {
                flac: Some(flac),
                ffmpeg: None,
            }),
            None => Err(ConvertError::NoConverter(Box::new(observation.clone()))),
        };
    }
    match encode(
        &observation.ffmpeg.executable,
        &ffmpeg_arguments(wav, &output),
        &output,
    ) {
        Ok(()) => Ok((output, ConversionOutcome::FfmpegFallback)),
        Err(failure) => {
            remove_failed_output(&output)?;
            Err(ConvertError::Encode {
                flac: flac_failure,
                ffmpeg: Some(failure),
            })
        }
    }
}

/// The converter that made a file, or the reason for no conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConversionOutcome {
    /// The file was not a WAV file.
    #[default]
    NotRequired,
    /// `flac` made the file.
    Flac,
    /// `ffmpeg` made the file.
    FfmpegFallback,
    /// The app kept the WAV file. The app sets this value.
    WavRetained,
}

/// The failure of one converter. It keeps no output of the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeFailure {
    /// The program did not start.
    Start(io::ErrorKind),
    /// The program stopped with this exit code, or with a signal (`None`).
    Exit(Option<i32>),
    /// The program stopped with code 0, but the output does not start as a
    /// FLAC file (R-CONVERT-2 item 3).
    NotFlac,
}

impl fmt::Display for EncodeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(kind) => write!(f, "the program did not start ({kind})"),
            Self::Exit(Some(code)) => write!(f, "exit {code}"),
            Self::Exit(None) => f.write_str("a signal stopped the program"),
            Self::NotFlac => f.write_str("the output is not a FLAC file"),
        }
    }
}

/// The error of a conversion. The WAV file stays (R-CONVERT-5).
#[derive(Debug)]
pub enum ConvertError {
    /// A file is at the FLAC path. The converter does not replace it.
    OutputExists(PathBuf),
    /// No converter passed its probe (R-CONVERT-6).
    NoConverter(Box<ConverterObservation>),
    /// Each converter that the function tried failed. `None` for a converter
    /// that the function did not try.
    Encode {
        /// The failure of `flac`.
        flac: Option<EncodeFailure>,
        /// The failure of `ffmpeg`.
        ffmpeg: Option<EncodeFailure>,
    },
    /// A file operation failed.
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        source: io::Error,
    },
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputExists(path) => {
                write!(f, "conversion output already exists: {}", path.display())
            }
            Self::NoConverter(observation) => write!(
                f,
                "no converter passed its version check: FLAC {} ({:?}, {:?}); ffmpeg {} ({:?}). The WAV file stays.",
                observation.flac.executable.display(),
                observation.flac.source,
                observation.flac.outcome,
                observation.ffmpeg.executable.display(),
                observation.ffmpeg.outcome
            ),
            Self::Encode { flac, ffmpeg } => match (flac, ffmpeg) {
                (Some(flac), Some(ffmpeg)) => write!(
                    f,
                    "FLAC encoding failed ({flac}); the ffmpeg fallback also failed ({ffmpeg})"
                ),
                (Some(flac), None) => write!(f, "FLAC encoding failed ({flac})"),
                (None, Some(ffmpeg)) => write!(f, "ffmpeg encoding failed ({ffmpeg})"),
                (None, None) => f.write_str("encoding failed"),
            },
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ConvertError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The arguments of `flac`: the best compression, no output text, and
/// replace a file at the output path.
fn flac_arguments(wav: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "--best".into(),
        "--silent".into(),
        "--totally-silent".into(),
        "-f".into(),
        "-o".into(),
        output.into(),
        wav.into(),
    ]
}

/// The arguments of `ffmpeg`. The output is 24-bit FLAC, so a 32-bit float
/// WAV file keeps 24 bits (operator change to musicindex ADR 0094 R-CONVERT-2
/// item 2). The FLAC encoder of `ffmpeg` keeps 24 bits in 32-bit samples.
fn ffmpeg_arguments(wav: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-i".into(),
        wav.into(),
        "-sample_fmt".into(),
        "s32".into(),
        "-bits_per_raw_sample".into(),
        "24".into(),
        "-compression_level".into(),
        "12".into(),
        output.into(),
    ]
}

/// Starts `program` with `arguments` and makes sure that `output` starts as
/// a FLAC file. The output of the program goes nowhere.
fn encode(program: &Path, arguments: &[OsString], output: &Path) -> Result<(), EncodeFailure> {
    let status = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| EncodeFailure::Start(error.kind()))?;
    if !status.success() {
        return Err(EncodeFailure::Exit(status.code()));
    }
    match AudioFormat::detect_from_file(output) {
        Ok(AudioFormat::Flac) => Ok(()),
        _ => Err(EncodeFailure::NotFlac),
    }
}

fn remove_failed_output(path: &Path) -> Result<(), ConvertError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ConvertError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// Writes `script` as a shell program at `path`.
    pub(crate) fn executable(path: &Path, script: &str) {
        fs::write(path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    /// A `flac` script that writes `bytes` to the path after `-o`.
    fn flac_writing(bytes: &str) -> String {
        format!(
            "if [ \"$1\" = --version ]; then exit 0; fi\nwhile [ \"$1\" != -o ]; do shift; done\nshift\nprintf '{bytes}' > \"$1\""
        )
    }

    /// An `ffmpeg` script that writes `bytes` to its last argument.
    fn ffmpeg_writing(bytes: &str) -> String {
        format!(
            "if [ \"$1\" = -version ]; then exit 0; fi\nfor last do :; done\nprintf '{bytes}' > \"$last\""
        )
    }

    /// The probe of an `ffmpeg` script at `path`. The encode starts the
    /// same path.
    fn ffmpeg_at(path: &Path) -> ConverterProbe {
        ConverterProbe::run(path, ExecutableSource::Path, "-version", None)
    }

    fn observation(flac: &Path, ffmpeg: &Path) -> ConverterObservation {
        ConverterObservation {
            flac: ConverterProbe::flac(Some(flac)),
            ffmpeg: ConverterProbe::flac(Some(ffmpeg)),
        }
    }

    /// R-CONVERT-2 item 4 and R-CONVERT-5 item 1: a failed `flac` leaves no
    /// output, `ffmpeg` makes the file, and the WAV file stays. The error
    /// keeps no output of the program.
    #[test]
    fn adr_0066_conversion_actual_fallback_cleans_partial_output_and_keeps_input() {
        let temp = tempfile::tempdir().unwrap();
        let wav = temp.path().join("input.wav");
        let output = wav.with_extension("flac");
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        let flac = temp.path().join("flac");
        let ffmpeg = temp.path().join("ffmpeg");
        executable(
            &flac,
            "[ \"$1\" = --version ] && exit 0\nwhile [ \"$1\" != -o ]; do shift; done\nshift\nprintf partial > \"$1\"\nexit 7",
        );
        executable(
            &ffmpeg,
            "[ \"$1\" = --version ] && exit 0\nfor last do :; done\n[ -e \"$last\" ] && exit 99\nprintf 'fLaC-fallback' > \"$last\"",
        );
        let observation = observation(&flac, &ffmpeg);
        let (path, outcome) = convert_with(&wav, &observation).unwrap();
        assert_eq!(path, output);
        assert_eq!(outcome, ConversionOutcome::FfmpegFallback);
        assert!(wav.exists());
        fs::remove_file(&output).unwrap();
        executable(
            &ffmpeg,
            "for last do :; done\nprintf partial > \"$last\"\nprintf 'fixture secret' >&2\nexit 8",
        );
        let error = convert_with(&wav, &observation).unwrap_err();
        assert!(matches!(
            error,
            ConvertError::Encode {
                flac: Some(EncodeFailure::Exit(Some(7))),
                ffmpeg: Some(EncodeFailure::Exit(Some(8))),
            }
        ));
        assert!(!error.to_string().contains("fixture secret"));
        assert!(!output.exists());
        assert!(wav.exists());
    }

    /// R-CONVERT-2 items 1 and 2, R-CONVERT-3 and R-CONVERT-5 item 2.
    #[test]
    fn adr_0066_converter_preserves_flac_first_and_existing_ffmpeg_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let flac = temp.path().join("flac");
        let ffmpeg = temp.path().join("ffmpeg");
        let wav = temp.path().join("track.wav");
        let output = wav.with_extension("flac");
        // The output bytes show which converter made the file.
        executable(&flac, &flac_writing("fLaC-reference"));
        executable(&ffmpeg, &ffmpeg_writing("fLaC-fallback"));
        let check = |path: &Path| ConverterObservation {
            flac: ConverterProbe::flac(Some(path)),
            ffmpeg: ffmpeg_at(&ffmpeg),
        };
        let replace = |observation: &ConverterObservation| {
            let (path, _) = convert_with(&wav, observation)?;
            fs::remove_file(&wav).unwrap();
            Ok::<_, ConvertError>(path)
        };
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        replace(&check(&flac)).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"fLaC-reference");
        assert!(!wav.exists());
        // A configured `flac` that fails on the input still lets `ffmpeg` run.
        fs::remove_file(&output).unwrap();
        executable(&flac, "if [ \"$1\" = --version ]; then exit 0; fi\nexit 9");
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        replace(&check(&flac)).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"fLaC-fallback");
        // A configured path that does not exist stays missing. `ffmpeg` runs.
        fs::remove_file(&output).unwrap();
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        let missing = check(&temp.path().join("explicit-missing"));
        assert_eq!(missing.flac.outcome, ProbeOutcome::Missing);
        replace(&missing).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"fLaC-fallback");
        executable(&ffmpeg, "exit 8");
        fs::remove_file(&output).unwrap();
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        assert!(replace(&check(&temp.path().join("explicit-missing"))).is_err());
        assert_eq!(fs::read(&wav).unwrap(), b"RIFFfixtureWAVE");
    }

    /// R-CONVERT-3: with a configured `flac` path that does not exist and a
    /// `flac` on `PATH`, the converter does not start the `flac` on `PATH`.
    #[test]
    fn a_configured_flac_path_is_not_replaced_by_path() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("path-flac-ran");
        executable(
            &temp.path().join("flac"),
            &format!("touch '{}'\nexit 0", marker.display()),
        );
        let observation = ConverterObservation {
            flac: ConverterProbe::flac_with_path(
                Some(&temp.path().join("missing-flac")),
                Some(temp.path()),
            ),
            ffmpeg: ConverterProbe::ffmpeg_with_path(Some(temp.path())),
        };
        let wav = temp.path().join("song.wav");
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        let error = convert_with(&wav, &observation).unwrap_err();
        assert!(matches!(error, ConvertError::NoConverter(_)), "{error}");
        assert!(!marker.exists(), "R-CONVERT-3: the flac on PATH ran");
    }

    /// R-CONVERT-2 items 3 and 4: an output that does not start as a FLAC
    /// file gives an error, and the output is gone.
    #[test]
    fn an_output_that_is_not_flac_is_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let flac = temp.path().join("flac");
        executable(&flac, &flac_writing("RIFFnot-flac"));
        let observation = ConverterObservation {
            flac: ConverterProbe::flac(Some(&flac)),
            ffmpeg: ConverterProbe::ffmpeg_with_path(Some(temp.path())),
        };
        let wav = temp.path().join("song.wav");
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        let error = convert_with(&wav, &observation).unwrap_err();
        assert!(matches!(
            error,
            ConvertError::Encode {
                flac: Some(EncodeFailure::NotFlac),
                ffmpeg: None,
            }
        ));
        assert!(!wav.with_extension("flac").exists());
        assert!(wav.exists());
    }

    /// R-CONVERT-6: with no converter, the function gives the typed error.
    #[test]
    fn no_converter_is_a_typed_error() {
        let temp = tempfile::tempdir().unwrap();
        let observation = ConverterObservation {
            flac: ConverterProbe::flac_with_path(None, Some(temp.path())),
            ffmpeg: ConverterProbe::ffmpeg_with_path(Some(temp.path())),
        };
        let wav = temp.path().join("song.wav");
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        let Err(ConvertError::NoConverter(seen)) = convert_with(&wav, &observation) else {
            panic!("R-CONVERT-6: no converter must give ConvertError::NoConverter");
        };
        assert_eq!(seen.flac.outcome, ProbeOutcome::Missing);
        assert_eq!(seen.ffmpeg.outcome, ProbeOutcome::Missing);
        assert!(wav.exists());
    }

    /// Operator change to R-CONVERT-2 item 2: `ffmpeg` writes 24-bit FLAC.
    /// The script records the arguments that `ffmpeg` gets.
    #[test]
    fn ffmpeg_asks_for_24_bit_flac() {
        let temp = tempfile::tempdir().unwrap();
        let record = temp.path().join("arguments");
        let ffmpeg = temp.path().join("ffmpeg");
        executable(
            &ffmpeg,
            &format!(
                "if [ \"$1\" = -version ]; then exit 0; fi\necho \"$@\" > '{}'\nfor last do :; done\nprintf 'fLaC' > \"$last\"",
                record.display()
            ),
        );
        let observation = ConverterObservation {
            flac: ConverterProbe::flac_with_path(None, Some(&temp.path().join("none"))),
            ffmpeg: ffmpeg_at(&ffmpeg),
        };
        let wav = temp.path().join("float.wav");
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        let (_, outcome) = convert_with(&wav, &observation).unwrap();
        assert_eq!(outcome, ConversionOutcome::FfmpegFallback);
        let arguments = fs::read_to_string(record).unwrap();
        assert!(
            arguments.contains("-sample_fmt s32 -bits_per_raw_sample 24"),
            "musicindex ADR 0094 R-CONVERT-2 item 2: ffmpeg must write 24-bit FLAC: {arguments}"
        );
    }

    /// R-CONVERT-5 item 1: `convert_retaining_input` keeps the WAV file, and
    /// a file at the FLAC path stops the conversion.
    #[test]
    fn a_file_at_the_flac_path_stays() {
        let temp = tempfile::tempdir().unwrap();
        let wav = temp.path().join("song.wav");
        fs::write(&wav, b"RIFFfixtureWAVE").unwrap();
        fs::write(wav.with_extension("flac"), b"old").unwrap();
        let error = convert_retaining_input(&wav, Some(&temp.path().join("missing"))).unwrap_err();
        assert!(matches!(error, ConvertError::OutputExists(_)));
        assert_eq!(fs::read(wav.with_extension("flac")).unwrap(), b"old");
        assert!(wav.exists());
    }
}
