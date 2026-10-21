# Fuzzing (T3)

Every reader and parser that takes untrusted input (definition packs, ROMs,
traces, device streams) has a fuzz target. A target fails when an input makes
it **panic**, **hang**, or **allocate without bound**. Story US-RR-5;
[PROJECT.md Testing Strategy](../.project/PROJECT.md#testing-strategy) (T3),
[SECURITY.md](../.project/SECURITY.md) (input handling).

## Layout

```
fuzz/                        cargo-fuzz crate `rr-fuzz` (its own workspace, nightly only)
  Cargo.toml                 [[bin]] per target + [package.metadata.rr-fuzz] limits
  src/lib.rs                 target bodies: pub fn <name>(data: &[u8]), and TARGETS
  src/replay.rs              stable replay of regressions/ (child process per input)
  tests/regressions.rs       the replay test (custom harness): self-test, then replay
  fuzz_targets/<name>.rs     one-line fuzz_target! wrapper around rr_fuzz::<name>
  seeds/<name>/              committed starting inputs (small, valid-ish)
  regressions/<name>/        committed minimised crash inputs (replayed in ci.yml);
                             every target has one (.gitkeep until its first crash)
  corpus/<name>/             growing corpus: git-ignored, cached by the nightly job
  artifacts/<name>/          crash / timeout / oom inputs libFuzzer saved: git-ignored
```

Targets (`cargo xtask fuzz list`):

| Target   | Under test | Input | Seeds |
|----------|------------|-------|-------|
| `ecu_id` | `rr_core::capability` ECU ids and address ranges (the template) | raw bytes | hand-picked |
| `expr`   | `rr_core::expr`: parse within default, tightened and raised `Limits`, print back, evaluate through `eval` and `eval_with` (US-RR-20-9) | formula text, then optionally `NUL`, a limit-scale byte and little-endian `f64`s for `x` and the parameters (`fuzz/src/expr_target.rs`) | every formula of `fixtures/kat/expr-*.toml` (`kat-*`, kept in sync by `xtask/tests/fuzz.rs`) plus formulas at each parser limit (`*-at-limit`); `regressions/expr/` holds inputs just over each limit |
| `pattern` | `rr_core::pattern`: parse byte pattern text (`@0x2 80 F0 ?? 01`), print back, match at the pattern's offset, at bases up to `usize::MAX` and by `find` (US-RR-23-5) | pattern text, then optionally `NUL` and the bytes to match (`fuzz/src/pattern_target.rs`) | hand-picked: each text form, matches and past-the-end, and malformed text of each kind |
| `toy`    | nothing: planted bugs for `cargo xtask fuzz selftest` | length-prefixed records | hand-picked |
| `xml`    | `rr_defs::xml`: read under the default `Limits` (input capped at 1 MiB, so a legitimately large tree cannot trip the memory limits) and under tight ones (depth 4, 16 elements, ...), check every diagnostic and the tree against the limits, write the tree back out and re-read it (US-RR-27-7) | one XML file, raw bytes (`fuzz/src/xml_target.rs`) | hand-written, real-shaped ECU, logger and XDF definitions (UTF-8, BOM + CRLF, ISO-8859-1) and one broken file; `regressions/xml/` is the hostile sample set of `crates/rr-defs/tests/data/xml/hostile/` byte for byte (billion laughs, XXE, deep nesting, ...; kept identical by the fuzz crate's `xml_regressions_are_the_hostile_set` test) |

`fuzz/` is **not** part of the root workspace (root `Cargo.toml` has
`exclude = ["fuzz"]`, and `fuzz/Cargo.toml` declares its own `[workspace]` and
`Cargo.lock`). Stable `cargo build/clippy/nextest --workspace` never builds it,
and the architecture test's "only three binaries" rule, `cargo xtask
check-deps` and `cargo deny check` apply to the root workspace only. The fuzz
crate's only third-party dependency is `libfuzzer-sys` (plus its build
helpers); everything else is workspace crates under test, by path. Its
licence, `(MIT OR Apache-2.0) AND NCSA` (NCSA from LLVM's libFuzzer,
GPL-compatible), is not on deny.toml's allowlist, so `cargo deny
--manifest-path fuzz/Cargo.toml check` reports it; the fuzz binaries are never
shipped.

Target bodies live in `src/lib.rs`, not in `fuzz_targets/`, so the same code
runs without libFuzzer: `cargo test --manifest-path fuzz/Cargo.toml` (stable)
replays every seed through its target, and committed crash inputs under
`regressions/<name>/` are replayed through `rr_fuzz::TARGETS` by `cargo xtask
fuzz replay` and the `fuzz-regressions` job in `ci.yml` (see [Regression
replay](#regression-replay-ciyml)).

## Setup

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
```

The workspace itself stays on the stable toolchain from `rust-toolchain.toml`;
only cargo-fuzz runs on nightly. Set `RR_FUZZ_TOOLCHAIN=nightly-YYYY-MM-DD` to
pin a dated nightly.

`cargo xtask fuzz run` always passes `--target <host>` (the `host:` of
`rustc +nightly -vV`). Without it cargo-fuzz builds for the triple it was
itself compiled for, and the prebuilt cargo-fuzz (what `cargo binstall` and
CI install) is `x86_64-unknown-linux-musl`, where the sanitizer cannot link
("sanitizer is incompatible with statically linked libc"). Calling `cargo
fuzz` directly with a prebuilt cargo-fuzz needs the same `--target`.

## Running

Always go through the wrapper, which applies the limits (below):

```sh
cargo xtask fuzz list                                 # targets and limits
cargo xtask fuzz run ecu_id --max-total-time 60       # fuzz for 60 s
cargo xtask fuzz run ecu_id                           # until a failure or Ctrl-C
cargo xtask fuzz run ecu_id --input fuzz/artifacts/ecu_id/crash-<hash>   # reproduce
cargo xtask fuzz run ecu_id -- -jobs=4 -workers=4     # extra libFuzzer flags after --
```

`fuzz run` is `cargo +nightly fuzz run --fuzz-dir fuzz <target>
fuzz/corpus/<target> fuzz/seeds/<target> -- -timeout=… -rss_limit_mb=…
-malloc_limit_mb=… [your flags]`. Plain `cargo +nightly fuzz run <target> --
-max_total_time=N` also works from the repository root, but runs with
libFuzzer's own, much looser defaults (1200 s per input) and without the seeds.

A failing run exits non-zero, prints the libFuzzer report and the panic or
stack, and writes the input to `fuzz/artifacts/<target>/` (`crash-*`,
`timeout-*` or `oom-*`).

## Limits (default on)

`fuzz/Cargo.toml`:

```toml
[package.metadata.rr-fuzz]
timeout_secs = 10      # -timeout: one input running longer is a failure
rss_limit_mb = 2048    # -rss_limit_mb: process RSS above this is a failure
malloc_limit_mb = 512  # -malloc_limit_mb: one allocation above this is a failure
```

`cargo xtask fuzz run` passes all three on every run, for every target,
before any flags you add. libFuzzer takes the last value of a repeated flag, so
a flag after `--` may *tighten* a limit (`-- -timeout=3`), but `fuzz run`
refuses to start on one that loosens or disables it (`-timeout=0`, a value
above the committed one, a non-integer) or that would let a failing input pass
(`-ignore_timeouts`, `-ignore_ooms`, `-ignore_crashes`, `-timeout_exitcode`,
`-error_exitcode`). The nightly job runs every target through the wrapper and
`xtask/tests/fuzz_ci_config.rs` fails if it calls `cargo fuzz` directly or
passes a limit flag itself. `cargo xtask fuzz check`, which the xtask
test suite runs in CI (`xtask/tests/fuzz.rs`), fails if any limit is missing,
zero, or out of range (timeout 1–60 s, RSS 256–4096 MB, allocation cap at most
the RSS cap). The file is code-owned (`.github/CODEOWNERS`).

`cargo xtask fuzz selftest` proves the limits bite. It builds the `toy`
target with its planted bugs and runs one input each, under the committed
limits:

| Case                                    | Input        | Must be reported as                        |
|-----------------------------------------|--------------|--------------------------------------------|
| clean input                             | `\x02ok\x00` | (passes)                                   |
| panic                                   | `PANIC`      | `panicked … planted panic`                 |
| hang (`timeout_secs`)                   | `HANG`       | `libFuzzer: timeout`                       |
| oversized allocation (`malloc_limit_mb`)| `OOM-MALLOC` | `libFuzzer: out-of-memory (malloc(…))`     |
| unbounded memory growth (`rss_limit_mb`)| `OOM-RSS`    | `libFuzzer: out-of-memory (used: …)`       |

The planted bugs exist only behind the `planted-panic`, `planted-hang` and
`planted-oom` features of the fuzz crate; never enable them for a real run.
`cargo xtask fuzz run toy --features planted-panic` finds the panic from the
seeds within seconds, which is how the nightly job's issue filing is tested.

## Adding a target

Use `ecu_id` (the template) as the model. For a target `defs_xml`:

1. **Body.** In `fuzz/src/lib.rs` add `pub fn defs_xml(data: &[u8])`. Call the
   API under test on `data` (decode bytes with `std::str::from_utf8` first if
   it takes `&str`, and `return` on invalid UTF-8). Then `assert!` the
   properties that must hold for every input it accepts (round-trips, bounds,
   lengths). Returning an error is fine. Panicking is the bug.
2. **Register it.** Add `("defs_xml", defs_xml)` to `TARGETS` in the same file.
3. **Wrapper.** Copy `fuzz/fuzz_targets/ecu_id.rs` to
   `fuzz/fuzz_targets/defs_xml.rs` and change the call to
   `rr_fuzz::defs_xml(data)`.
4. **Manifest.** In `fuzz/Cargo.toml` copy the `ecu_id` `[[bin]]` block,
   changing `name` and `path`. If the API lives in another workspace crate,
   add it under `[dependencies]` by path (`rr-defs = { path =
   "../crates/rr-defs" }`), then `cargo +nightly fuzz build --fuzz-dir fuzz`
   updates `fuzz/Cargo.lock`. Commit it.
5. **Seeds.** Add a few small real inputs to `fuzz/seeds/defs_xml/` (a minimal
   valid file, an edge case, one known-bad one). Keep them small and free of
   private data (no customer ROMs).
6. **Regressions directory.** Create `fuzz/regressions/defs_xml/` with an
   empty `.gitkeep` in it (git does not keep empty directories; the replay
   skips that file). Crash inputs go here later (see [When the fuzzer finds a
   bug](#when-the-fuzzer-finds-a-bug)).
7. **Check and run.**

   ```sh
   cargo xtask fuzz check
   cargo xtask fuzz replay                           # seeds and regressions, stable
   cargo xtask fuzz run defs_xml --max-total-time 60
   ```

`fuzz check` fails until steps 2–6 are all done.

## When the fuzzer finds a bug

The nightly job files an issue with these steps (`cargo xtask fuzz
file-issues`); the same applies to a crash found locally.

1. Reproduce: `cargo xtask fuzz run <target> --input fuzz/artifacts/<target>/<file>`.
2. Minimise: `cargo +nightly fuzz tmin --fuzz-dir fuzz <target> <file>`. It
   writes the smallest input that still fails to
   `fuzz/artifacts/<target>/minimized-from-<hash>` (a timeout or OOM input
   minimises the same way; pass the limits with `-- -timeout=10` etc. if it
   needs them).
3. Commit it as `fuzz/regressions/<target>/<short-name>`, a name that says
   what it is (`empty-range-overflow`, not the hash):

   ```sh
   mkdir -p fuzz/regressions/<target>
   cp fuzz/artifacts/<target>/minimized-from-<hash> fuzz/regressions/<target>/<short-name>
   cargo xtask fuzz replay       # must FAIL now, naming <target>/<short-name>
   ```

   Files there are raw bytes (`fuzz/.gitattributes` marks them binary); keep
   them small (`xtask/tests/fuzz.rs` rejects anything over 64 KiB) and free of
   private data.
4. Fix the bug. `cargo xtask fuzz replay` passes again, and from then on the
   input is an ordinary regression test in `ci.yml`. Add a unit or property
   test in the owning crate too if the failure has a clear rule behind it.

Commit the input together with the fix (or with the fix to follow in the same
pull request): a committed input that still fails turns `ci-ok` red.

## Regression replay (ci.yml)

`cargo xtask fuzz replay` is `cargo test --manifest-path fuzz/Cargo.toml
--locked` on the **stable** toolchain from `rust-toolchain.toml`: no nightly,
no cargo-fuzz, no libFuzzer. The `fuzz-regressions` job in `ci.yml` runs it on
every push and pull request, and `ci-ok` needs it (wiring checked by
`xtask/tests/fuzz_replay_ci_config.rs`). It runs, in order:

1. the fuzz crate's unit tests, including every seed through its target;
2. `fuzz/tests/regressions.rs` (a custom harness, `harness = false`):
   * **self-test**: on scratch regression trees, the replay must fail on a
     planted panic, a planted hang (killed at a 2 s test timeout), a planted
     oversized allocation, planted unbounded heap growth, a directory for an
     unknown target, a stray file and a nested directory, and must pass on a
     clean input, an empty target directory and no directory at all. If the
     replay stopped catching any of these, the job fails;
   * **replay**: every file in `fuzz/regressions/<target>/` through
     `rr_fuzz::<target>`, each in its **own child process** (the test binary
     re-run with `RR_FUZZ_REPLAY_TARGET` / `RR_FUZZ_REPLAY_INPUT`), under the
     limits in `fuzz/Cargo.toml`: the child is killed after `timeout_secs`
     (10 s), one allocation over `malloc_limit_mb` or a live heap over
     `rss_limit_mb` aborts it (a counting allocator; heap bytes, not RSS). A
     panic, abort, signal or timeout fails that input; the others still run
     and the output of each failure is printed.

A hang cannot stall CI: the parent polls the child (`Child::try_wait`) and
kills it at `timeout_secs` (`Child::kill`), and the job has
`timeout-minutes: 20` as a backstop. Both the timeout and the allocation caps
use only `std` (no `cfg(unix)` code), so the replay behaves the same on
Windows, where `Child::kill` is `TerminateProcess` and a refused allocation's
abort exits with a non-zero code. CI runs the replay on Linux only; on Windows
it is supported but not exercised by CI.

Layout rules (also checked statically by `cargo xtask fuzz check`):

| Under `fuzz/regressions/`              | Result                                   |
|----------------------------------------|------------------------------------------|
| no `regressions/` directory            | ok, nothing to replay                    |
| a target with no directory, or empty   | ok, nothing to replay for it (but `fuzz check` requires the directory) |
| `<target>/.gitkeep`                    | skipped: it only keeps the directory in git |
| `<target>/<file>`                      | replayed through that target             |
| `<name>/` that is not a target         | **fails**: rename it or delete it        |
| a file directly in `regressions/`      | **fails**: it would never be replayed    |
| a directory inside `<target>/`         | **fails**: it would never be replayed    |

So renaming or removing a target without moving its regressions fails CI
rather than silently dropping them.

The committed `fuzz/regressions/toy/` inputs (`planted-panic`, `planted-hang`,
`planted-oom-malloc`) pass, because the toy target is correct without its
planted features; they show the replay catches each kind of bug in a real
target body:

```sh
cargo xtask fuzz replay                           # ok
cargo xtask fuzz replay --features planted-panic  # fails: toy/planted-panic panicked
cargo xtask fuzz replay --features planted-hang   # fails: toy/planted-hang timed out after 10 s
cargo xtask fuzz replay --features planted-oom    # fails: toy/planted-oom-malloc aborted
```

`toy/planted-panic` (`PANIC`) is exactly what the nightly job's own
planted-panic find (run 39317525293, input `PANIC\x04$`) minimises to with
`cargo +nightly fuzz tmin --features planted-panic`, so the committed example
is the real loop's output, not a hand-made stand-in (US-RR-5-3).

No input for a real target is committed that still fails.

## Nightly CI

`.github/workflows/fuzz.yml` (US-RR-5-6), nightly at 02:41 UTC and on manual
dispatch:

1. **`targets`** builds the matrix with `cargo xtask fuzz list --json`, so a
   new `[[bin]]` is fuzzed from its first night with no workflow edit.
2. **`fuzz`** (one leg per target, `fail-fast: false`) runs
   `cargo xtask fuzz run <target> --max-total-time 600`: ten minutes, through
   the wrapper, so the limits above apply. The corpus `fuzz/corpus/<target>`
   is restored from the newest cache of that target (`restore-keys:
   fuzz-corpus-<target>-`) and saved under a new key after every run, failing
   ones included, so it keeps growing. On failure the leg uploads artifact
   `fuzz-failure-<target>`: the inputs libFuzzer saved (`crash-*`,
   `timeout-*`, `oom-*`), `fuzz.log` and `features.txt`.
3. **`report`** runs only if a leg failed, and is the only job whose token
   can write (`issues: write`). It downloads the artifacts and runs
   `cargo xtask fuzz file-issues --dir fuzz-failures --run-url <run>`
   (`xtask/src/fuzz_issue.rs`), which opens one issue labelled `fuzz` per
   target titled ``fuzz: target `<target>` found a failing input``, or, when
   an open issue already has that title, comments on it. The body has the
   kind of failure, the saved inputs (size, first bytes in hex), a log
   excerpt and the reproduce/minimise steps below. Crash data goes to `gh`
   in a body file, never on a command line or through `${{ }}` in a script.
   A leg that failed without saving an input (a build break) files nothing;
   the red run reports it.

Manual dispatch inputs: `target` (one target; empty means all), `seconds`
(default 600), `planted_panic`, and `dry_run_issues` (print the `gh` calls
instead of running them). To see the whole chain work, dispatch with
`target: toy` and `planted_panic: true`: the leg fails within seconds and an
issue titled ``fuzz: target `toy` found a failing input (features:
planted-panic)`` is filed (its own title, so a self-test never lands on a
real finding). Close it once seen.

Locally, the filing step can be rehearsed without touching GitHub:

```sh
mkdir -p /tmp/f/fuzz-failure-toy
echo planted-panic > /tmp/f/fuzz-failure-toy/features.txt
cargo xtask fuzz run toy --features planted-panic --max-total-time 60 \
  --artifact-prefix /tmp/f/fuzz-failure-toy/ 2>&1 | tee /tmp/f/fuzz-failure-toy/fuzz.log
cargo xtask fuzz file-issues --dir /tmp/f --dry-run    # prints the gh calls and the body
```

The wiring is checked by `xtask/tests/fuzz_ci_config.rs`, the filing by
`xtask/tests/fuzz_issue.rs`.
