# RomRaider-RS — Infrastructure

This is a desktop application, so there are no servers. "Infrastructure" here means the dev environment, CI, the test corpus, the hardware bench and the release pipeline.

## Environments

| Environment | URL | Purpose | Notes |
|------------|-----|---------|-------|
| Development | local | Dev on Linux and Windows | `cargo nextest run` with public fixtures. Private fixtures via `RR_FIXTURES_DIR`. |
| CI | GitHub Actions on https://github.com/Biztactix-Ryan/RomRaider | Gates every push and PR | Hosted runners. A self-hosted HIL runner comes later. |
| Hardware bench (HIL) | local / self-hosted runner | Real adapter + car ECU | Initially Ryan's 2004 Subaru Liberty (AU, NA) via a USB KKL cable, SSM over K-line. **Read-only tests only.** No bench ECU yet, so write-path HIL is blocked until one exists. |
| Nightly channel | GitHub Releases (pre-release) | Builds from `rust-rewrite` / main | Opt-in testers |
| Stable channel | GitHub Releases | Tagged `vX.Y.Z` releases | From M4 onwards |

## CI/CD

### Build Pipeline

GitHub Actions workflows (`.github/workflows/`):

| Workflow | Trigger | Jobs |
|----------|---------|------|
| `ci.yml` | push, PR | `fmt` + `clippy -D warnings`; `nextest` matrix (see below); `insta` snapshot check (no pending snapshots); `cargo-deny` (licences, advisories, bans); T6 safety-invariant suite; stock-ROM corpus job `corpus` (see "Stock-ROM corpus in CI" below); aggregate check `ci-ok` |
| `coverage.yml` | push to `master` / `rust-rewrite`, PR | `cargo-llvm-cov`; fails if rr-core/rr-rom/rr-comms < 90% or a PR regresses coverage; check `coverage-gate` |
| `mutants.yml` | every PR (job `scope` skips the gate jobs unless gated crates, test support or the gate changed; incremental `--in-diff`); weekly full run | `cargo-mutants` on encoding, expr, journal, checksum, write gating; 0 survivors; aggregate check `mutants-ok` (always runs) |
| `fuzz.yml` | nightly | Each `cargo-fuzz` target for 10 min; corpus cached; crashes opened as issues |
| `fixtures.yml` | every push and PR (job `scope` skips the fixture jobs unless `fixtures/`, `rr-testkit`, `xtask` or the build config changed) | KAT schema validation; every expected value has a source; synthetic ROMs regenerate byte-identically; ROM-leak guard; aggregate check `fixtures-ok` (always runs) |
| `corpus-cold.yml` | weekly schedule, on demand | Cold `fetch-corpus` of the stock-ROM corpus into a fresh directory with no Actions cache, against the same 5-minute budget as `ci.yml` job `corpus` (see "Stock-ROM corpus in CI"); not a required check |
| `release.yml` | tag `v*` / nightly schedule | `cargo-dist` builds and uploads artifacts |

**Required status checks** (branch protection; CONTRIBUTING.md "Branch protection (maintainers)", verified by `cargo xtask check-branch-protection`): `ci-ok` on `master` and `rust-rewrite`; on `rust-rewrite` also `coverage-gate`, `mutants-ok` and `fixtures-ok`. Every required check comes from a job that reports on every pull request. A path-filtered workflow never starts on other changes and would leave its check at "Expected", so `mutants.yml` and `fixtures.yml` have no trigger-level `paths:`. Their first job, `scope`, does a `git diff` of the change against the gate's path list. `coverage.yml`'s `gate` (`coverage-gate`) also runs `if: always()` and fails unless both coverage runs succeeded, so it is never skipped. The aggregate job runs `if: always()`: it fails when `scope` failed, a gate job failed or was cancelled, or a job that should have run was skipped, and passes when the change is out of scope (`xtask/tests/{mutants,fixtures}_ci_config.rs`, `xtask/tests/common/scoped_gate.rs`).

**Stock-ROM corpus in CI** (`ci.yml` job `corpus`, US-RR-11-8, D-025; wiring checked by `xtask/tests/corpus_ci_config.rs`):
- `RR_CORPUS_DIR` is `$HOME/.cache/romraider-rs/corpus`, the same default a developer gets: outside the workspace (`fetch-corpus` refuses a directory inside the repo), and on a persistent self-hosted runner it stays warm between jobs (`runner.temp` is wiped).
- `actions/cache/restore` keys it `rr-corpus-v1-<runner.os>-<hashFiles('fixtures/corpus-manifest.toml')>`, with no `restore-keys`. The manifest is LF-pinned (`fixtures/.gitattributes`), so the key is the same on every checkout. Bump `v1` to drop every cached corpus.
- `cargo xtask fetch-corpus` always runs: on a hit it verifies the restored tree and fetches nothing (~1 s); on a miss, or a hit that does not verify, it fetches the pinned commit from public upstream (no secrets, so fork PRs work too). Since US-RR-16-5 the same step also fetches and verifies the pinned ECU definitions from public GitHub (TD-D/SubaruDefs, D-028) into `RR_CORPUS_DIR/defs/`, which the same cache entry holds; the manifest that keys the cache pins both, so a new definitions pin is a new cache key. The logger package is never in CI (D-028): the step logs `logger v370: not present` and carries on. A miss is saved with `actions/cache/save` straight after the verified fetch, never on a hit. About 411 MB (ROMs) plus 239 MB (definitions) on disk, well under the 10 GB repository cache limit. The `Verify all corpus images` step's `--verify-only` checks the definitions too and fails if they do not verify.
- `Corpus coverage report` (US-RR-16-6) then runs `cargo xtask gen-corpus-coverage --check` on the fetched corpus: it fails unless `fixtures/corpus-coverage.toml` is byte-identical to the report generated from the pinned ROMs and definitions (every file it reads is hash-checked against the manifest first).
- **Cold-fetch budget: 5 minutes** (the fetch step's `timeout-minutes`, repeated as `BUDGET_SECONDS: 300`). Measured cold fetches take 10–35 s locally (about 78 MB downloaded), so the budget leaves roughly 10x headroom for a slow GitHub or upstream day while still failing a hung fetch in minutes rather than at the 30-minute job timeout. xtask is built in an earlier step, so the time is the fetch alone. The step writes cache hit, cold/warm and elapsed time to the job log (`::notice`), the step summary and its outputs (`cache-hit`, `fetch`, `seconds`), and fails if the fetch went over budget.
- Once the cache is warm, `ci.yml` only sees cache hits until the manifest changes, so the budget is also checked weekly by `corpus-cold.yml` (US-RR-11-4; Wednesdays 04:29 UTC and on demand): it runs the same fetch step script (kept byte-identical to ci.yml's by `xtask/tests/corpus_cold_ci_config.rs`) into a freshly removed `$RUNNER_TEMP/rr-corpus-cold`, with no `actions/cache` step at all, and fails over budget or unless the fetch was cold. Like `mutants.yml`, its schedule fires only once the file is on the default branch.

**Test matrix:**
- `x86_64-unknown-linux-gnu`: tier 1.
- `x86_64-pc-windows-msvc`: tier 1.
- `i686-pc-windows-msvc`: many vendor J2534 DLLs are 32-bit only.
- `aarch64-unknown-linux-gnu`: tier 2. Raspberry Pi / ARM SBC as a headless in-car logger running `romraider-cli`.
- `aarch64-apple-darwin`: best effort.

Linux builds are linked against glibc 2.28 (via `cargo-zigbuild`), so one binary runs on Ubuntu 20.04+, Debian 10+, RHEL/Alma/Rocky 8+, Fedora, Arch and Mint without rebuilding.

The upstream Java CI workflow has been removed. No Java runs in CI (D-024).

### Deployment Process

1. Merge to the release branch with all gates green.
2. Run T9 hardware-in-loop by hand on the bench (read-only suite at minimum) and record the result in the release notes.
3. Tag `vX.Y.Z`. `release.yml` builds every artifact in the table below and publishes a GitHub Release with SHA-256 checksums.
   - Windows: MSI.
   - macOS: dmg (best effort).
   - Linux: see "Linux Release" below.
4. Code signing: Windows Authenticode and macOS notarization are TBD (ask: Ryan, whether a certificate is available).

### Rollback Procedure

- Previous releases stay downloadable on GitHub Releases. Users reinstall the prior version.
- The app never auto-updates silently. A future update check only notifies.
- Settings and profile files carry `schema = N`. A newer version never rewrites a file in a way an older version can't read without first writing a `.bak` of the old-schema file.

## Linux Release

**Linux is a first-class release target, equal to Windows (D-022).** The goal: download, run, plug in the cable and it works, with no Java, no `chmod`, no editing groups, no hunting for `.so` files.

### Artifacts

| Artifact | For | How it's built | Notes |
|----------|-----|----------------|-------|
| **AppImage** (x86_64, aarch64) | Any distro, "just download and run" | `appimagetool` in `release.yml` | Primary download. Includes `.desktop` + icon. On first run, the in-app device check offers to install the udev rules (see below). |
| **.deb** (amd64, arm64) | Debian, Ubuntu, Mint, Pop!_OS, Raspberry Pi OS | `cargo-deb` | Installs the binaries, desktop entry, icon, and the udev rules into `/usr/lib/udev/rules.d/`. Reloads udev in postinst. |
| **.rpm** (x86_64, aarch64) | Fedora, RHEL/Alma/Rocky, openSUSE | `cargo-generate-rpm` | Same contents as the .deb |
| **AUR `romraider-rs-bin`** | Arch, Manjaro, EndeavourOS | PKGBUILD in `packaging/aur/`, updated by the release workflow | Wraps the release tarball |
| **tar.gz** | Everything else, power users | `cargo-dist` | Contains binaries, `install.sh` (user-local into `~/.local`, or system-wide with `--system`), udev rules, desktop file |
| **Flatpak** | Deferred | — | The sandbox needs `--device=all` for serial and USB access, which defeats the point. Revisit if Flathub's portal story for USB improves. |

Binary names:
- `romraider-rs` is the desktop app.
- `romraider-cli` is the headless tool. **Not** `rr`, which collides with the widely-installed Mozilla `rr` debugger on Linux.

### Device access without fuss

On Linux the main friction points are permissions and other daemons grabbing the adapter. We ship `packaging/linux/60-romraider.rules`:
- **`TAG+="uaccess"`** for known adapter VID:PIDs: FTDI FT232R KKL cables (`0403:6001`) and CH340 KKL cables (`1a86:7523`, also used by ELM327 clones) first, because that's the target setup; then Tactrix OpenPort 2.0, CP210x and PL2303. This grants the logged-in desktop user access via systemd-logind. **No `dialout`/`uucp` group membership and no logout/login needed.**
- **`ENV{ID_MM_DEVICE_IGNORE}="1"`** on the same devices, so ModemManager doesn't probe `/dev/ttyACM*` / `/dev/ttyUSB*` with AT commands and corrupt the K-line session.
- The exact VID:PID list is verified against real hardware in M2. The rules file is generated from the same adapter profiles the app uses (C1), so there's one source of truth.

The in-app **device check** (also `romraider-cli doctor`) detects and explains, with the exact fix:
- `EACCES` on the port → offers to install the udev rules via `pkexec`. AppImage and tarball users get this; packages already have them.
- `brltty` holding a CH340/FTDI device (a common Ubuntu 22.04+ problem with cheap cables) → explains and offers the one-line fix.
- ModemManager active on the port, adapter not enumerated, or the J2534 library missing.

### Native libraries
- No native libraries are bundled.
- The `serialport` crate is built without libudev (`default-features = false`) and enumerates ports via sysfs, so there's no runtime libudev dependency.
- The GUI talks to X11 and Wayland directly through eframe/winit (both enabled). It uses the `glow` (OpenGL) renderer for the widest GPU compatibility.
- **J2534 on Linux:** the Java build ships `lib/linux/*/j2534.so`, an OpenPort 2.0 J2534 implementation over libusb. Plan (D-023): implement the OpenPort 2.0 USB protocol natively as a Rust Link (`nusb`, pure Rust, no libusb), so Linux, macOS and Windows can use an OP2 without any vendor library. Until then we load an external `j2534.so` from the paths listed in SECURITY.md.

### Package smoke tests (CI)
`release.yml` installs each package in a clean container and runs:
- `romraider-cli --version`
- `romraider-cli doctor --no-devices`
- a fixture decode

Containers:
- Ubuntu 20.04 / 22.04 / 24.04
- Debian 12
- Fedora (latest)
- Rocky 8
- Arch

The AppImage is smoke-tested on Ubuntu 20.04 (oldest supported glibc) and Fedora latest. A packaging regression fails the release.

## Hosting & Services

- **Source and CI:** GitHub (fork `Biztactix-Ryan/RomRaider`, branch `rust-rewrite`).
- **Distribution:** GitHub Releases.
- **External stock-ROM corpus (D-025):** https://github.com/samuelw9999/RomRaider, pinned at commit `639f3c73`.
  - About 500 stock Subaru ROMs: ADM 87, EDM 76, JDM 114, USDM 221.
  - Raw binary despite the `.hex` extension; 512 KB / 1 MB images.
  - **Local mirror:** `/home/ryan/Repo/romraider-stock-roms/` (bare `mirror.git`, `roms/` checkout, `SHA256SUMS`), taken 2026-10-19. **Private backup:** https://github.com/Biztactix-Ryan/romraider-stock-roms (private, full `git push --mirror`, created and verified 2026-10-19: fresh clone matches all 501 SHA-256s). It is a manual backup only: CI fetches from public upstream (decision, Ryan, 2026-10-20: no private mirror in CI for now, so no secrets are needed).
  - Fetched by `cargo xtask fetch-corpus` into `RR_CORPUS_DIR` (default `~/.cache/romraider-rs/corpus`) and verified against `fixtures/corpus-manifest.toml` (path, size, SHA-256, ROM ID). Cached in CI (see "Stock-ROM corpus in CI").
- **ECU definition corpus (D-028):** the `RomRaider/` subtree of https://github.com/TD-D/SubaruDefs (branch `Alpha`), pinned at commit `0b0c0c42`. Logger definitions: the full contract is the romraider.com forum package `logger_v370.zip` (986,211 bytes, SHA-256 `280d3865…`), downloaded once manually by Ryan (no scripted forum/Cloudflare bypass) and kept in a private mirror only; `fetch-corpus` takes it from that mirror or a local path in `RR_CORPUS_DIR`, hash-verifies it, and reports clearly if it is absent. Public CI uses TD-D's v290 logger files from the pinned tree. Neither source has a licence, so both are manifested like the ROMs and never committed (fetching and manifesting: US-RR-16-5; coverage report: US-RR-16-6).
  - 267 files, 239,467,067 bytes (about 229 MiB; the whole repository at the pin packs to about 30 MB, so the shallow fetch is quick). Four file names contain spaces (`ecu/RR D2UJ001T.xml`, `ecu/2004USDMBajaXTAT-A2WC50  .xml`, ...); they are kept exactly as upstream spells them.
  - **Manifest:** the `[defs]` table of `fixtures/corpus-manifest.toml` (US-RR-16-5): upstream, branch, commit, `path_prefix = "RomRaider/"`, the licence position, every file's path (relative to the prefix), size and SHA-256, and `[defs.logger_v370]` (file name, size, SHA-256, forum URL for people only). Metadata only. `cargo xtask gen-corpus-manifest --mirror <ROM mirror> --defs-mirror <defs mirror>` regenerates the whole file and `--check` verifies it.
  - **Coverage report:** `fixtures/corpus-coverage.toml` (US-RR-16-6), generated by `cargo xtask gen-corpus-coverage` from a fetched corpus (`--check` in CI): for each of the 501 images, every definition file whose `<romid>` matches the image bytes the way legacy RomRaider matches (`internalidaddress` + `internalidstring`, `base` resolved within the file) with its `xmlid`, or `no-definition`; the ROM ID read from the image; and the 2004 ADM Liberty flags. Metadata only. At the pins: 456 matched, 45 no definition, 0 unresolved base; by `filename_rom_id` 455 (D-028's figure). All eight 2004 ADM Liberty images are covered; no 2004 ADM Liberty 2.0i/2.5i is in either corpus.
  - **Local mirror:** `/home/ryan/Repo/romraider-ecu-defs/` (bare `git clone --mirror` of TD-D/SubaruDefs in `mirror.git`, every branch; `SHA256SUMS` of the 267 raw blobs at the pin, paths relative to `RomRaider/`), taken 2026-10-21. It is a valid `--defs-source`. **Private backup:** none yet (follow-up for Ryan: push `mirror.git` to a private repo like the ROM backup, and keep the hand-downloaded `logger_v370.zip` in a private mirror, D-028).
  - **Cache layout:** `fetch-corpus` keeps the definitions inside `RR_CORPUS_DIR`, in its top-level `defs/` directory (never a ROM path): `defs/romraider/` is the verified copy of the subtree (its own `.rr-defs-cache` marker), and `defs/logger_v370.zip` is where a hand-placed copy of the logger package is looked for. ROM verification ignores `defs/`, and replacing a bad ROM cache carries it over. The logger package is never downloaded or copied: `--logger-zip <path>` / `RR_LOGGER_ZIP` points at a copy (e.g. in the private mirror), else `defs/logger_v370.zip` is checked; absent is reported and the run carries on, a size or SHA-256 mismatch fails the run.
- **Private fixture corpus:** Ryan's own ROM(s) and car traces, stored outside the public repo. Location TBD (ask: Ryan; options are a private repo with Git LFS or a Biztactix file share). Mounted locally via `RR_FIXTURES_DIR`.
- **Upstream sync:** `git fetch upstream && git merge upstream/master` lands Java changes. Git rename detection maps them into `legacy/`. Treat upstream Java fixes as potential bug reports: if one changes behaviour we rely on, add or update a known-answer fixture.

## Monitoring & Alerting

- No telemetry. The app never phones home (see SECURITY.md).
- CI failures notify through GitHub's standard notifications to repo watchers.
- Nightly fuzz crashes are auto-filed as GitHub issues labelled `fuzz`.
- Users diagnose problems locally: `RR_LOG=debug` plus `.rrtrace` comms captures. Users can attach traces to issues; they replay as tests.

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `RR_LOG` | No | `tracing` filter, e.g. `rr_comms=trace` |
| `RR_CORPUS_DIR` | No | Cache directory for the external stock-ROM and definition corpora (default `~/.cache/romraider-rs/corpus`; definitions in its `defs/`) |
| `RR_CORPUS_SOURCE` | No | `fetch-corpus`: git URL or path to fetch the ROMs from instead of the manifest's upstream (e.g. `/home/ryan/Repo/romraider-stock-roms/mirror.git`) |
| `RR_DEFS_SOURCE` | No | `fetch-corpus`: git URL or path to fetch the ECU definitions from instead of the manifest's upstream (e.g. `/home/ryan/Repo/romraider-ecu-defs/mirror.git`) |
| `RR_LOGGER_ZIP` | No | `fetch-corpus`: path to the hand-downloaded `logger_v370.zip` (e.g. in the private mirror) to verify; else `RR_CORPUS_DIR/defs/logger_v370.zip` is checked. Never set in CI. |
| `RR_CORPUS_MIRROR` / `RR_DEFS_MIRROR` | No | `gen-corpus-manifest`: the local ROM and definitions mirrors the manifest is generated from |
| `RR_FIXTURES_DIR` | No | Path to the private fixture corpus. Enables `--features private-fixtures` tests. |
| `RR_HIL_PORT` | No | Serial port for hardware-in-loop tests. HIL is skipped if unset. |
| `RR_HIL_J2534` | No | J2534 library name/path for HIL tests |
| `RR_HIL_ALLOW_WRITE` | No | Must be set (and on a bench ECU only) to run write-path HIL tests |
| `RR_J2534_LIB` | No | Overrides registry-based J2534 DLL discovery at runtime |

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
