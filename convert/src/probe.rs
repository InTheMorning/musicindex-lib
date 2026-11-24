//! The version probe of `flac` and `ffmpeg` (musicindex ADR 0094 R-CONVERT-4).

use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The time limit of one probe.
pub const CONVERTER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The output limit of one probe in bytes, for stdout and stderr together.
pub const CONVERTER_PROBE_OUTPUT_CAP: usize = 16 * 1024;
const PROBE_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Where the probe found the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutableSource {
    /// The app gave the path.
    Configured,
    /// The name on `PATH`.
    Path,
}

/// The result of one probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The program stopped with this exit code. `None` when a signal
    /// stopped it.
    Exited(Option<i32>),
    /// The program does not exist.
    Missing,
    /// The program cannot be started because of its permissions.
    PermissionDenied,
    /// The program did not stop in [`CONVERTER_PROBE_TIMEOUT`]. The probe
    /// stopped it.
    TimedOut,
    /// The program wrote [`CONVERTER_PROBE_OUTPUT_CAP`] bytes or more. The
    /// probe stopped it.
    OutputLimit,
    /// A different I/O error.
    IoFailure(io::ErrorKind),
}

/// The probe of one program. It keeps no output of the program, because a
/// program can write a secret (musicindex ADR 0094 R-CONVERT-4 item 3).
#[derive(Clone, Debug)]
pub struct ConverterProbe {
    /// The program that the probe started.
    pub executable: PathBuf,
    /// Where the probe found the program.
    pub source: ExecutableSource,
    /// The time of the probe.
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    /// The result.
    pub outcome: ProbeOutcome,
}

impl ConverterProbe {
    /// Probes `flac --version`. `configured` is the path that the app gives.
    /// When it is `None`, the probe uses `flac` on `PATH`. A configured path
    /// is never replaced with `flac` on `PATH` (R-CONVERT-3).
    #[must_use]
    pub fn flac(configured: Option<&Path>) -> Self {
        Self::flac_with_path(configured, None)
    }

    /// Probes `ffmpeg -version` on `PATH`.
    #[must_use]
    pub fn ffmpeg() -> Self {
        Self::ffmpeg_with_path(None)
    }

    /// `true` when the program stopped with exit code 0.
    #[must_use]
    pub fn available(&self) -> bool {
        self.outcome == ProbeOutcome::Exited(Some(0))
    }

    /// [`Self::flac`] with `search_path` as `PATH` of the program. The tests
    /// use it.
    pub(crate) fn flac_with_path(configured: Option<&Path>, search_path: Option<&Path>) -> Self {
        Self::run(
            configured.unwrap_or_else(|| Path::new("flac")),
            if configured.is_some() {
                ExecutableSource::Configured
            } else {
                ExecutableSource::Path
            },
            "--version",
            search_path,
        )
    }

    pub(crate) fn ffmpeg_with_path(search_path: Option<&Path>) -> Self {
        Self::run(
            Path::new("ffmpeg"),
            ExecutableSource::Path,
            "-version",
            search_path,
        )
    }

    pub(crate) fn run(
        executable: &Path,
        source: ExecutableSource,
        argument: &str,
        search_path: Option<&Path>,
    ) -> Self {
        let mut command = Command::new(executable);
        command.arg(argument);
        if let Some(search_path) = search_path {
            command.env("PATH", search_path);
        }
        let outcome = bounded_probe(&mut command).unwrap_or_else(|error| match error.kind() {
            io::ErrorKind::NotFound => ProbeOutcome::Missing,
            io::ErrorKind::PermissionDenied => ProbeOutcome::PermissionDenied,
            kind => ProbeOutcome::IoFailure(kind),
        });
        Self {
            executable: executable.into(),
            source,
            recorded_at: chrono::Utc::now(),
            outcome,
        }
    }
}

/// The probes of the two converters.
#[derive(Clone, Debug)]
pub struct ConverterObservation {
    /// The probe of `flac`.
    pub flac: ConverterProbe,
    /// The probe of `ffmpeg`.
    pub ffmpeg: ConverterProbe,
}

impl ConverterObservation {
    /// Probes the two programs again. Each call replaces the earlier result,
    /// also a result that found no program.
    #[must_use]
    pub fn refresh(configured: Option<&Path>) -> Self {
        Self {
            flac: ConverterProbe::flac(configured),
            ffmpeg: ConverterProbe::ffmpeg(),
        }
    }
}

fn bounded_probe(command: &mut Command) -> io::Result<ProbeOutcome> {
    // A socket that does not block needs no reader thread. A reader thread
    // can stay after the child stops, when a child of the child keeps stdout
    // open. stdout and stderr use one byte limit.
    let (mut reader, writer) = UnixStream::pair()?;
    reader.set_nonblocking(true)?;
    let stderr = writer.try_clone()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .stderr(Stdio::from(OwnedFd::from(stderr)));
    let mut child = command.spawn()?;
    let deadline = Instant::now() + CONVERTER_PROBE_TIMEOUT;
    let result = observe_child(&mut child, &mut reader, deadline);
    // try_wait collects a child that stopped. For each other result, the
    // probe stops and collects the child before it returns, also after a
    // read error or a wait error.
    if !matches!(result, Ok(ProbeOutcome::Exited(_))) {
        let _ = child.kill();
        child.wait()?;
    }
    result
}

fn observe_child(
    child: &mut Child,
    reader: &mut UnixStream,
    deadline: Instant,
) -> io::Result<ProbeOutcome> {
    let mut consumed = 0;
    let mut buffer = [0u8; 1024];
    let mut exited = None;
    loop {
        loop {
            if Instant::now() >= deadline {
                return Ok(ProbeOutcome::TimedOut);
            }
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    consumed += count;
                    if consumed >= CONVERTER_PROBE_OUTPUT_CAP {
                        return Ok(ProbeOutcome::OutputLimit);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(status) = exited {
            return Ok(ProbeOutcome::Exited(status));
        }
        if let Some(status) = child.try_wait()? {
            exited = Some(status.code());
            continue;
        }
        std::thread::sleep(PROBE_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::executable;

    #[test]
    fn adr_0066_converter_path_checks_refresh_in_one_process() {
        let temp = tempfile::tempdir().unwrap();
        for (name, probe) in [
            (
                "flac",
                (|path: &Path| ConverterProbe::flac_with_path(None, Some(path)))
                    as fn(&Path) -> ConverterProbe,
            ),
            ("ffmpeg", |path: &Path| {
                ConverterProbe::ffmpeg_with_path(Some(path))
            }),
        ] {
            let check = || probe(temp.path());
            assert_eq!(check().outcome, ProbeOutcome::Missing);
            let path = temp.path().join(name);
            executable(&path, "exit 0");
            let success = check();
            assert!(success.available());
            std::fs::remove_file(path).unwrap();
            let missing = check();
            assert_eq!(missing.outcome, ProbeOutcome::Missing);
            assert!(missing.recorded_at >= success.recorded_at);
        }
        let first = temp.path().join("first");
        executable(&first, "exit 0");
        assert!(ConverterProbe::flac(Some(&first)).available());
        let second = temp.path().join("second");
        let changed = ConverterProbe::flac(Some(&second));
        assert_eq!(changed.executable, second);
        assert_eq!(changed.source, ExecutableSource::Configured);
        assert_eq!(changed.outcome, ProbeOutcome::Missing);
    }

    /// musicindex ADR 0094 R-CONVERT-4: each probe result is separate, and
    /// the probe keeps no output.
    #[test]
    fn adr_0066_converter_failures_are_distinct_and_output_is_not_retained() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("flac");
        executable(&path, "printf 'token=never-report-this' >&2\nexit 7");
        let failed = ConverterProbe::flac(Some(&path));
        assert_eq!(failed.outcome, ProbeOutcome::Exited(Some(7)));
        assert!(!format!("{failed:?}").contains("never-report-this"));
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        assert_eq!(
            ConverterProbe::flac(Some(&path)).outcome,
            ProbeOutcome::PermissionDenied
        );
        executable(
            &path,
            "while :; do printf '0123456789012345678901234567890123456789'; done",
        );
        let start = Instant::now();
        assert_eq!(
            ConverterProbe::flac(Some(&path)).outcome,
            ProbeOutcome::OutputLimit
        );
        assert!(start.elapsed() < CONVERTER_PROBE_TIMEOUT);
    }

    /// musicindex ADR 0094 R-CONVERT-4 item 2: a program that does not stop
    /// ends after 5 s, and the probe collects it.
    #[test]
    fn adr_0066_converter_timeout_reaps_child() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("flac");
        let pid = temp.path().join("pid");
        executable(
            &path,
            &format!("echo $$ > '{}'\nexec /bin/sleep 30", pid.display()),
        );
        let start = Instant::now();
        assert_eq!(
            ConverterProbe::flac(Some(&path)).outcome,
            ProbeOutcome::TimedOut
        );
        assert!(start.elapsed() < CONVERTER_PROBE_TIMEOUT + Duration::from_secs(2));
        #[cfg(target_os = "linux")]
        assert!(
            !Path::new("/proc")
                .join(std::fs::read_to_string(pid).unwrap().trim())
                .exists()
        );
    }
}
