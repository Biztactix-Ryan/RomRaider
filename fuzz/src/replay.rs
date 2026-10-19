//! Replay of committed crash inputs on stable, without libFuzzer (task
//! US-RR-5-7; docs/fuzzing.md "When the fuzzer finds a bug").
//!
//! Every file under `fuzz/regressions/<target>/` is a minimised input that
//! once made `<target>` fail. The `regressions` test
//! (`fuzz/tests/regressions.rs`, run by `cargo xtask fuzz replay` and the
//! `fuzz-regressions` job in ci.yml) feeds each one to the target body from
//! [`crate::TARGETS`] and fails if any input panics, runs past the per-input
//! timeout, or allocates past the memory caps. Each input runs in its **own
//! child process** (the test binary re-executed with [`ENV_TARGET`] and
//! [`ENV_INPUT`] set), so:
//!
//! * a hang is killed after [`Limits::timeout`] and reported, instead of
//!   hanging CI until the job timeout;
//! * a panic, an abort, a stack overflow or a refused allocation in one input
//!   is reported against that input and the others still run.
//!
//! The limits are the ones libFuzzer gets (`fuzz/Cargo.toml`
//! `[package.metadata.rr-fuzz]`, [`limits_from_manifest`]): `timeout_secs`
//! per input; `malloc_limit_mb` for one allocation and `rss_limit_mb` for the
//! live heap, both enforced in the child by the test binary's counting
//! allocator (heap bytes, not RSS: there is no portable RSS cap in std).
//!
//! The layout is checked deliberately ([`discover`]):
//!
//! * no `regressions/` directory, or a target with no directory: nothing to
//!   replay, which passes;
//! * a directory whose name is not a target: a failure (a renamed or removed
//!   target would otherwise silently stop replaying its crashes);
//! * a plain file directly under `regressions/`, or a directory inside a
//!   target's directory: a failure (it would never be replayed).

use std::fmt;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use rr_core::clock::{Clock, RealClock};

/// Set in the child: the target to run.
pub const ENV_TARGET: &str = "RR_FUZZ_REPLAY_TARGET";
/// Set in the child: the input file.
pub const ENV_INPUT: &str = "RR_FUZZ_REPLAY_INPUT";
/// Set in the child: the largest single allocation, in bytes.
pub const ENV_MALLOC_LIMIT: &str = "RR_FUZZ_REPLAY_MALLOC_LIMIT";
/// Set in the child: the live-heap cap, in bytes.
pub const ENV_HEAP_LIMIT: &str = "RR_FUZZ_REPLAY_HEAP_LIMIT";

/// Per-input limits for a replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Wall-clock time one input may run before its child is killed.
    pub timeout: Duration,
    /// Live-heap cap in the child, in MB (`rss_limit_mb`).
    pub heap_limit_mb: u64,
    /// Largest single allocation in the child, in MB (`malloc_limit_mb`).
    pub malloc_limit_mb: u64,
}

/// Reads `timeout_secs`, `rss_limit_mb` and `malloc_limit_mb` from the
/// `[package.metadata.rr-fuzz]` table of `fuzz/Cargo.toml` text. Only the
/// `key = <integer>` lines of that table are read (the crate has no TOML
/// parser); `cargo xtask fuzz check` validates the table with a real one.
///
/// # Errors
///
/// Names the first missing or non-integer key, or a zero value.
pub fn limits_from_manifest(text: &str) -> Result<Limits, String> {
    Ok(Limits {
        timeout: Duration::from_secs(table_int(text, "timeout_secs")?),
        heap_limit_mb: table_int(text, "rss_limit_mb")?,
        malloc_limit_mb: table_int(text, "malloc_limit_mb")?,
    })
}

/// The positive integer `want` in `[package.metadata.rr-fuzz]` of `text`.
fn table_int(text: &str, want: &str) -> Result<u64, String> {
    let mut in_table = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            in_table = line == "[package.metadata.rr-fuzz]";
            continue;
        }
        let Some((k, v)) = line.split_once('=').filter(|_| in_table) else {
            continue;
        };
        if k.trim() == want {
            return match v.trim().parse::<u64>() {
                Ok(0) => Err(format!("[package.metadata.rr-fuzz] {want} is 0")),
                Ok(n) => Ok(n),
                Err(_) => Err(format!(
                    "[package.metadata.rr-fuzz] {want} = {} is not an integer",
                    v.trim()
                )),
            };
        }
    }
    Err(format!("[package.metadata.rr-fuzz] has no {want}"))
}

/// One committed input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    /// Target name (the directory under `regressions/`).
    pub target: String,
    /// The file.
    pub path: PathBuf,
}

impl Input {
    /// `<target>/<file name>`, for messages.
    #[must_use]
    pub fn label(&self) -> String {
        let file = self
            .path
            .file_name()
            .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
        format!("{}/{file}", self.target)
    }
}

/// The inputs under `dir` (a `regressions/` directory) for the targets
/// named in `targets`, sorted by target then file name, or every layout
/// problem found (see the module docs). A missing `dir`, or a target with no
/// directory, contributes no inputs.
///
/// # Errors
///
/// Returns the layout problems, one message each; also an unreadable
/// directory.
pub fn discover(dir: &Path, targets: &[&str]) -> Result<Vec<Input>, Vec<String>> {
    let mut problems = Vec::new();
    let mut inputs = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(vec![format!("{}: {e}", dir.display())]),
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if !path.is_dir() {
            problems.push(format!(
                "regressions/{name}: a file directly under regressions/ is never replayed; \
                 put it in regressions/<target>/"
            ));
            continue;
        }
        if !targets.contains(&name.as_str()) {
            problems.push(format!(
                "regressions/{name}/: unknown fuzz target {name:?} (targets: {}); \
                 rename the directory to its target or delete it",
                targets.join(", ")
            ));
            continue;
        }
        let files = match fs::read_dir(&path) {
            Ok(f) => f,
            Err(e) => {
                problems.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let mut files: Vec<PathBuf> = files.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for file in files {
            if file.is_dir() {
                problems.push(format!(
                    "regressions/{name}/{}/: subdirectories are not replayed; \
                     inputs go directly in regressions/{name}/",
                    file.file_name().unwrap_or_default().to_string_lossy()
                ));
                continue;
            }
            inputs.push(Input {
                target: name.clone(),
                path: file,
            });
        }
    }
    if problems.is_empty() {
        Ok(inputs)
    } else {
        Err(problems)
    }
}

/// How one input's child process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Exited successfully: the target accepted or rejected the input
    /// without failing.
    Passed,
    /// Exited unsuccessfully: a panic, an abort (refused allocation, stack
    /// overflow) or a signal. The text describes the exit status.
    Failed(String),
    /// Still running at the timeout, and killed.
    TimedOut(Duration),
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed => f.write_str("ok"),
            Self::Failed(s) => write!(f, "FAILED ({s})"),
            Self::TimedOut(t) => write!(f, "FAILED (timed out after {} s, killed)", t.as_secs()),
        }
    }
}

/// The result of one input.
#[derive(Debug, Clone)]
pub struct Replayed {
    /// The input.
    pub input: Input,
    /// How its child ended.
    pub outcome: Outcome,
    /// The child's stdout and stderr (panic message, allocation failure).
    pub output: String,
    /// How long it ran.
    pub elapsed: Duration,
}

/// A whole replay: layout problems and per-input results.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Layout problems from [`discover`]; any one fails the replay.
    pub problems: Vec<String>,
    /// One entry per input replayed.
    pub results: Vec<Replayed>,
}

impl Report {
    /// Whether the replay passed: no layout problem and every input passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.problems.is_empty() && self.results.iter().all(|r| r.outcome == Outcome::Passed)
    }

    /// The inputs that did not pass.
    pub fn failures(&self) -> impl Iterator<Item = &Replayed> {
        self.results.iter().filter(|r| r.outcome != Outcome::Passed)
    }
}

/// The command that replays `input` in a child: `exe` (the test binary
/// itself) with the [`ENV_TARGET`], [`ENV_INPUT`] and memory-limit variables
/// set.
#[must_use]
pub fn child_command(exe: &Path, input: &Input, limits: &Limits) -> Command {
    let mut cmd = Command::new(exe);
    cmd.env(ENV_TARGET, &input.target)
        .env(ENV_INPUT, &input.path)
        .env(ENV_MALLOC_LIMIT, (limits.malloc_limit_mb << 20).to_string())
        .env(ENV_HEAP_LIMIT, (limits.heap_limit_mb << 20).to_string())
        .env("RUST_BACKTRACE", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn describe(status: ExitStatus) -> String {
    match status.code() {
        Some(101) => "exit code 101: panicked".to_owned(),
        Some(c) => format!("exit code {c}"),
        None => format!("{status}"),
    }
}

/// Runs `cmd` to completion or until `timeout`, killing it then. Returns
/// how it ended, its combined output and the time it ran.
///
/// # Errors
///
/// Fails if the child cannot be started or waited for.
pub fn run_with_timeout(
    mut cmd: Command,
    timeout: Duration,
) -> std::io::Result<(Outcome, String, Duration)> {
    // Time through rr-core's RealClock, the only sanctioned OS clock (D-010).
    let clock = RealClock::new();
    let start = clock.now();
    let since = |c: &RealClock| c.now().saturating_duration_since(start);
    let mut child = cmd.spawn()?;
    // Drain both pipes on their own threads so a chatty child cannot block
    // on a full pipe while we wait for it.
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let outcome = loop {
        if let Some(status) = child.try_wait()? {
            break if status.success() {
                Outcome::Passed
            } else {
                Outcome::Failed(describe(status))
            };
        }
        if since(&clock) >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            break Outcome::TimedOut(timeout);
        }
        clock.sleep(Duration::from_millis(5));
    };
    let elapsed = since(&clock);
    let mut text = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    text.push_str(&String::from_utf8_lossy(&err.join().unwrap_or_default()));
    Ok((outcome, text, elapsed))
}

/// Replays every input under `dir` for `targets`, each in a child process
/// started from `exe` under `limits`.
///
/// # Errors
///
/// Fails only if a child cannot be started; layout problems and failing
/// inputs are in the [`Report`].
pub fn replay(
    exe: &Path,
    dir: &Path,
    targets: &[&str],
    limits: &Limits,
) -> std::io::Result<Report> {
    let inputs = match discover(dir, targets) {
        Ok(i) => i,
        Err(problems) => {
            return Ok(Report {
                problems,
                results: Vec::new(),
            });
        }
    };
    let mut results = Vec::with_capacity(inputs.len());
    for input in inputs {
        let (outcome, output, elapsed) =
            run_with_timeout(child_command(exe, &input, limits), limits.timeout)?;
        results.push(Replayed {
            input,
            outcome,
            output,
            elapsed,
        });
    }
    Ok(Report {
        problems: Vec::new(),
        results,
    })
}

/// What a child was asked to run: [`ENV_TARGET`] and [`ENV_INPUT`], and the
/// memory caps in bytes (0: none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildRequest {
    /// Target name.
    pub target: String,
    /// Input file.
    pub input: PathBuf,
    /// Largest single allocation, in bytes.
    pub malloc_limit: usize,
    /// Live-heap cap, in bytes.
    pub heap_limit: usize,
}

/// The child's request, if this process was started by [`child_command`].
#[must_use]
pub fn child_request() -> Option<ChildRequest> {
    let target = std::env::var(ENV_TARGET).ok()?;
    let input = std::env::var_os(ENV_INPUT)?.into();
    let bytes = |k: &str| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
    };
    Some(ChildRequest {
        target,
        input,
        malloc_limit: bytes(ENV_MALLOC_LIMIT),
        heap_limit: bytes(ENV_HEAP_LIMIT),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "[package]\nname = \"x\"\n[package.metadata.rr-fuzz]\n\
        timeout_secs = 10 # per input\nrss_limit_mb = 2048\nmalloc_limit_mb = 512\n\
        [features]\ntimeout_secs = 99\n";

    #[test]
    fn limits_come_from_the_rr_fuzz_table_only() {
        let l = limits_from_manifest(MANIFEST).unwrap();
        assert_eq!(l.timeout, Duration::from_secs(10));
        assert_eq!(l.heap_limit_mb, 2048);
        assert_eq!(l.malloc_limit_mb, 512);
        let missing = MANIFEST.replace("malloc_limit_mb = 512", "");
        assert!(
            limits_from_manifest(&missing)
                .unwrap_err()
                .contains("malloc_limit_mb")
        );
        let zero = MANIFEST.replace("timeout_secs = 10", "timeout_secs = 0");
        assert!(limits_from_manifest(&zero).unwrap_err().contains("is 0"));
    }

    #[test]
    fn committed_manifest_limits_parse() {
        let text = include_str!("../Cargo.toml");
        limits_from_manifest(text).unwrap();
    }
}
