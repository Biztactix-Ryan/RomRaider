# RomRaider-RS — Infrastructure

This is a desktop application, so there are no servers. "Infrastructure" here means the dev environment, CI, the test corpus, the hardware bench and the release pipeline.

## Environments

| Environment | URL | Purpose | Notes |
|------------|-----|---------|-------|
| Development | local | Dev on Linux and Windows | `cargo nextest run` with public fixtures. Private fixtures via `RR_FIXTURES_DIR`. |
| CI | GitHub Actions on https://github.com/Biztactix-Ryan/RomRaider | Gates every push and PR | Hosted runners. A self-hosted HIL runner comes later. |
| Hardware bench (HIL) | local / self-hosted runner | Real adapter + bench or car ECU | TBD (ask: Ryan, which ECU/cable/bench). Read-only tests by default. |
| Nightly channel | GitHub Releases (pre-release) | Builds from `rust-rewrite` / main | Opt-in testers |
| Stable channel | GitHub Releases | Tagged `vX.Y.Z` releases | From M4 onwards |

## CI/CD

### Build Pipeline

GitHub Actions workflows (`.github/workflows/`):

| Workflow | Trigger | Jobs |
|----------|---------|------|
| `ci.yml` | push, PR | `fmt` + `clippy -D warnings`; `nextest` matrix (see below); `insta` snapshot check (no pending snapshots); `cargo-deny` (licences, advisories, bans); T6 safety-invariant suite |
| `coverage.yml` | push to main, PR | `cargo-llvm-cov`; fails if rr-core/rr-rom/rr-comms < 90% or a PR regresses coverage |
| `mutants.yml` | PR touching gated modules (incremental `--in-diff`); weekly full run | `cargo-mutants` on encoding, expr, journal, checksum, write gating; 0 survivors |
| `fuzz.yml` | nightly | Each `cargo-fuzz` target for 10 min; corpus cached; crashes opened as issues |
| `legacy.yml` | push touching `legacy/` or `tools/fixture-recorder/` | Ant build of the Java oracle (`working-directory: legacy`); recorder compiles; re-recorded fixtures must match committed ones |
| `release.yml` | tag `v*` / nightly schedule | `cargo-dist` builds and uploads artifacts |

**Test matrix:**
- `x86_64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`
- `i686-pc-windows-msvc`: many vendor J2534 DLLs are 32-bit only.
- `aarch64-apple-darwin`: best effort, since J2534 doesn't exist on macOS.

The existing Java workflow (`.github/workflows/build.yaml`) has been retargeted at `legacy/`. It becomes `legacy.yml` in M0.

### Deployment Process

1. Merge to the release branch with all gates green.
2. Run T9 hardware-in-loop by hand on the bench (read-only suite at minimum) and record the result in the release notes.
3. Tag `vX.Y.Z`. `cargo-dist` builds per-target archives and installers (MSI on Windows, tar.gz/AppImage on Linux, dmg on macOS) and publishes a GitHub Release with checksums.
4. Code signing: Windows Authenticode and macOS notarization are TBD (ask: Ryan, whether a certificate is available).

### Rollback Procedure

- Previous releases stay downloadable on GitHub Releases. Users reinstall the prior version.
- The app never auto-updates silently. A future update check only notifies.
- Settings and profile files carry `schema = N`. A newer version never rewrites a file in a way an older version can't read without first writing a `.bak` of the old-schema file.

## Hosting & Services

- **Source and CI:** GitHub (fork `Biztactix-Ryan/RomRaider`, branch `rust-rewrite`).
- **Distribution:** GitHub Releases.
- **Private fixture corpus:** real ROMs and car traces, stored outside the public repo. Location TBD (ask: Ryan; options are a private repo with Git LFS or a Biztactix file share). Mounted locally via `RR_FIXTURES_DIR`.
- **Upstream sync:** `git fetch upstream && git merge upstream/master` lands Java changes. Git rename detection maps them into `legacy/`. Re-record fixtures if the oracle's behaviour changed.

## Monitoring & Alerting

- No telemetry. The app never phones home (see SECURITY.md).
- CI failures notify through GitHub's standard notifications to repo watchers.
- Nightly fuzz crashes are auto-filed as GitHub issues labelled `fuzz`.
- Users diagnose problems locally: `RR_LOG=debug` plus `.rrtrace` comms captures. Users can attach traces to issues; they replay as tests.

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `RR_LOG` | No | `tracing` filter, e.g. `rr_comms=trace` |
| `RR_FIXTURES_DIR` | No | Path to the private fixture corpus. Enables `--features private-fixtures` tests. |
| `RR_HIL_PORT` | No | Serial port for hardware-in-loop tests. HIL is skipped if unset. |
| `RR_HIL_J2534` | No | J2534 library name/path for HIL tests |
| `RR_HIL_ALLOW_WRITE` | No | Must be set (and on a bench ECU only) to run write-path HIL tests |
| `RR_J2534_LIB` | No | Overrides registry-based J2534 DLL discovery at runtime |
| `JAVA_HOME` | Recorder only | JDK for building `legacy/` and running the fixture recorder |

## Backup & Recovery

User data protection is built in, because user data is the only state that exists:
- **ROM saves:** atomic write (temp + rename), previous file kept as `.bak`. An optional sidecar journal (`<rom>.rrjournal`) keeps the edit history and any checksum overrides.
- **Settings and profiles:** atomic writes. A one-time import from Java `settings.xml` leaves the original untouched.
- **Logs:** CSV files are append-only and fsync'd on rotation and close.

Project data:
- **Source:** GitHub, plus local clones.
- **Private fixture corpus:** backup TBD (ask: Ryan). It must be backed up, because it can't be regenerated without the original cars and ROMs.

---
*Last reviewed: 2026-10-19*
