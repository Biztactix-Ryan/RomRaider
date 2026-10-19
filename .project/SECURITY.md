# RomRaider-RS — Security

This is an offline desktop app with no accounts or servers. The security model therefore covers three things:
1. **Physical safety.** Don't damage ECUs or engines.
2. **Untrusted input.** Definition packs, ROMs, XDFs, CSVs and traces downloaded from forums.
3. **Local integrity.** Native code loading, IPC and the supply chain.

## Authentication

### Method
None. There are no user accounts, logins or network services. The app makes no outbound network calls; a future update check will be opt-in and notify-only.

### Token/Session Management
N/A for users. The internal **capability tokens** that gate dangerous operations are described under Authorization.

### Multi-factor Authentication
N/A.

## Authorization

### Roles & Permissions

"Authorization" here means **what the software may do to hardware and files**, not what a person may do.

| Capability | Permits | How obtained |
|-----------|---------|--------------|
| (none, the default) | Read ROMs, decode, log read-only, replay, simulate | Always |
| `RomWrite` | Overwrite a ROM file on disk | Save action. Checksums must verify; otherwise an explicit "save with failing checksum" confirmation, recorded in the sidecar journal. |
| `EcuMemoryWrite` | SSM/KWP memory/address writes (global adjust, RAM tuning) | Explicit per-session confirmation naming the ECU ID and the address range |
| `EcuReset` | ECU reset, learning/adaptation reset | Explicit per-action confirmation |
| `EcuFlash` | Reflash | **Not implemented.** Out of scope until there is a HIL bench (VISION.md). |

The editor "user levels" (Beginner → Debug) only control which tables are visible. They are not a security boundary.

### Enforcement

- Write APIs in `rr-comms` and `rr-rom` take a capability token type that can only be built in `rr-app`/`rr-cli` confirmation paths. The type system enforces it: there is no public constructor.
- The T6 safety suite asserts that no code path reaches a Link write or file overwrite without a token. The Sim Link records every attempted write so tests can check for zero unauthorised writes.
- HIL write tests need `RR_HIL_ALLOW_WRITE` and run only against a bench ECU.

## Data Protection

### Encryption
- No data in transit (offline).
- At rest, files are plain. ROMs and logs belong to the user.
- Release artifacts ship with SHA-256 checksums, and are signed once certificates exist (INFRASTRUCTURE.md).

### PII Handling
- ECU traces and logs can contain **VINs, ECU serial and calibration IDs**, and timestamps that reveal driving patterns.
- `rr-cli trace scrub` and `rr-cli log scrub` redact VIN and serials before a user attaches files to a public issue. The issue template says so.
- Fixtures committed to the public repo are synthetic or scrubbed.

### Data Retention
- The app keeps nothing beyond what the user saves.
- No telemetry and no crash uploads. Crash info is written to a local file only.

### Proprietary firmware
- Real ECU ROM images are manufacturer-copyrighted.
- **They are never committed to our repo or shipped in our artifacts.**
- Public CI uses synthetic ROMs plus the external stock-ROM corpus (D-025). The corpus is fetched at test time from its own public repo and verified against our committed SHA-256 manifest.
- Real ROMs live in the private fixture corpus (`RR_FIXTURES_DIR`).
- A pre-commit hook and a CI check reject binaries over 64 KB, or files matching known ROM IDs, under `fixtures/public`.

## API Security

There is no network API. The equivalent is **input handling for untrusted files**:
- **XML (definitions, XDF):** no DTD processing, no external entities, limits on depth, attribute size and entity expansion. Readers are fuzzed (T3).
- **Formulas:** the Expr engine (C4) is pure: no I/O, no variables beyond declared refs, bounded recursion depth and AST size. Formulas from definitions can't do anything except compute numbers.
- **No runtime code loading of any kind.** This replaces Java's URLClassLoader that loaded checksum classes from the definition folder (MF-11), and the reflection-based plugin loading.
- **Binary inputs** (ROMs, traces, device streams): bounds-checked decoding, no panics, no unbounded allocation (fuzzed).
- **Native libraries:** J2534 libraries are never loaded from the CWD or the definition folders. The path loaded is always logged.
  - Windows: only paths registered under the `PassThruSupport.04.04` registry key, or an explicit `RR_J2534_LIB`.
  - Linux and macOS (no registry): a fixed, ordered list of system paths (`/usr/lib`, `/usr/local/lib`, `/usr/lib/<triple>`), plus `~/.local/lib/romraider-rs/`, plus an explicit `RR_J2534_LIB`.
  - The goal is to remove this on Linux entirely with the native OpenPort 2.0 Link (D-023).
- **Linux device access:** udev rules grant per-seat access via `TAG+="uaccess"` instead of adding the user to `dialout`. That gives access to the logged-in desktop user only, not to every process in a broad group. Installing rules from the AppImage goes through `pkexec`, showing the exact file contents before the user confirms.
- **Single-instance IPC:** a per-user Unix socket (0600) or named pipe (current-user ACL) carrying JSON-encoded args. This replaces the unauthenticated TCP port 23272 (MF-36).

## Secrets Management

The app itself holds no secrets.

Project secrets live in GitHub Actions encrypted secrets. Names (no values here):
- `WINDOWS_SIGNING_CERT`, `WINDOWS_SIGNING_PASSWORD`: TBD, if Authenticode is adopted
- `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_NOTARY_PASSWORD`: TBD, if macOS notarization is adopted
- `FIXTURE_CORPUS_TOKEN`: TBD, read-only access to the private corpus for a self-hosted runner

Access is limited to repo admins (Biztactix). Rotate on any maintainer change.

## Known Risks & Mitigations

| Risk | Severity | Mitigation | Status |
|------|----------|------------|--------|
| Bug writes wrong values to the ECU or a ROM (bricked ECU, engine damage) | Critical | Read-only default, capability tokens, checksum verify-before-save, out-of-range edits rejected, T1/T2/T6/T7 test tiers, HIL before release | Designed; built in M0–M2 |
| Wrong or stale live data leads to a bad tuning decision (MF-08/09/10) | High | `Option` values end to end, staleness TTLs, link-health indicator, T5 fault injection | Designed; M2–M3 |
| Malicious definition pack executes code | High | No code loading; pure Expr engine; fuzzed readers | Designed; M1 |
| Malicious J2534 DLL / DLL search-order hijack | Medium | Registry-registered paths only, never CWD; path logged | Designed; M2 |
| Proprietary ROM committed publicly | Medium | Size/ID check in pre-commit + CI; private corpus | Designed; M0 |
| VIN/serials leaked via attached traces | Low–Medium | Scrub commands, issue template guidance | Designed; M2 |
| Supply chain (malicious or vulnerable crate) | Medium | `cargo-deny` (advisories, licences, bans, sources), `Cargo.lock` committed, minimal deps | Designed; M0 |
| Local process abuses single-instance IPC | Low | Per-user socket/pipe with OS ACLs, JSON args | Designed; M4 |
| Overly broad Linux device permissions | Low | `uaccess` per-seat udev rules for known VID:PIDs only; no `dialout` group advice; `pkexec` install shows the rule contents | Designed; M2 |
| GPL compliance (derivative work) | Low | GPL-2.0-or-later licence, source published with each release, `cargo-deny` licence allowlist | Designed; M0 |

## Incident Response

- **Security vulnerability:** report via GitHub private security advisories on `Biztactix-Ryan/RomRaider`. Maintainer: Ryan (ryan@biztactix.com.au). Acknowledge within 7 days. Fix, release, then publish the advisory.
- **ECU-safety incident** (a release wrote bad data or showed wrong live data):
  1. Mark the release as withdrawn on GitHub Releases immediately.
  2. Publish an advisory naming the affected ECUs and operations.
  3. Add a failing test or trace reproducing it.
  4. Fix, then add a Must-Fix ledger entry and a DECISIONS entry if the design was at fault.
- **Upstream:** if the issue also exists in Java RomRaider, notify upstream maintainers via RomRaider's channels.

---
*Last reviewed: 2026-10-19*
