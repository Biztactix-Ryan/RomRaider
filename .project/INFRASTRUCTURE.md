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
| `ci.yml` | push, PR | `fmt` + `clippy -D warnings`; `nextest` matrix (see below); `insta` snapshot check (no pending snapshots); `cargo-deny` (licences, advisories, bans); T6 safety-invariant suite |
| `coverage.yml` | push to main, PR | `cargo-llvm-cov`; fails if rr-core/rr-rom/rr-comms < 90% or a PR regresses coverage |
| `mutants.yml` | PR touching gated modules (incremental `--in-diff`); weekly full run | `cargo-mutants` on encoding, expr, journal, checksum, write gating; 0 survivors |
| `fuzz.yml` | nightly | Each `cargo-fuzz` target for 10 min; corpus cached; crashes opened as issues |
| `fixtures.yml` | push touching `fixtures/` or `rr-testkit` | KAT schema validation; every expected value has a source; synthetic ROMs regenerate byte-identically; ROM-leak guard |
| `release.yml` | tag `v*` / nightly schedule | `cargo-dist` builds and uploads artifacts |

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
  - Fetched by `cargo xtask fetch-corpus` into `RR_CORPUS_DIR` (default `~/.cache/romraider-rs/corpus`) and verified against `fixtures/corpus-manifest.toml` (path, size, SHA-256, ROM ID). Cached in CI.
- **ECU definition corpus:** RomRaider ECU definitions matching the stock ROMs. Source TBD: find and pin it (story in E2). Fetched and manifested the same way.
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
| `RR_CORPUS_DIR` | No | Cache directory for the external stock-ROM and definition corpora (default `~/.cache/romraider-rs/corpus`) |
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
