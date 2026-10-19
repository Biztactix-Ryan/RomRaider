# Contributing to RomRaider-RS

RomRaider-RS is ECU software: a bug can brick an ECU or show wrong live data to someone tuning an engine. The rules below exist for that reason. They apply to every contributor, human or agent.

Read these first. They are the source of truth, and this file only summarises them:

- [VISION.md, Principles](.project/VISION.md#principles): the priority order that settles arguments.
- [PROJECT.md, Testing Strategy](.project/PROJECT.md#testing-strategy): test tiers T0–T10 and the testing rules.
- [ARCHITECTURE.md, Concept Catalogue](.project/ARCHITECTURE.md#concept-catalogue) and [Cross-cutting Standards](.project/ARCHITECTURE.md#cross-cutting-standards).
- [DECISIONS.md](.project/DECISIONS.md): every rule here cites a decision ID (`D-0xx`).

## Definition of Done

A change is done when **all** of the following hold. A PR that misses one is not ready to merge.

1. **Tests first, and in the right tiers.** Every behaviour change has tests in each tier that [applies to the crates it touches](#test-tiers-per-crate). A bug fix starts with a failing test, trace or fixture that reproduces it ([PROJECT.md rules](.project/PROJECT.md#testing-strategy), [D-003](.project/DECISIONS.md#d-003-fixture-first-multi-tier-testing)).
2. **Ledger entries closed properly.** Any [Must-Fix](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase) or [Edge-Case](.project/PROJECT.md#edge-cases-must-have-tests) entry the PR claims to close names its proving test, and that test exists and passes. See [Ledger rules](#ledger-rules-must-fix-and-edge-cases).
3. **No new concept without a decision.** Cross-crate types use an existing concept from the catalogue. A new one comes with a DECISIONS.md entry and a catalogue row. See [Concept catalogue rule](#concept-catalogue-rule).
4. **Every expected value has a source.** Fixtures and snapshots follow the [Fixture source rule](#fixture-source-rule).
5. **No Java.** Nothing in the change writes, builds or runs Java ([No-Java rule](#no-java-rule-d-024)).
6. **Safety-gated code gets the extra review.** Changes to write paths and gated modules follow the [Safety review process](#safety-review-process).
7. **All CI gates are green.** The required check is `ci-ok`; run the [local checks](#local-checks-before-a-pr) before pushing.
8. **Standards respected.** Errors, logging, threading and time follow [Cross-cutting Standards](.project/ARCHITECTURE.md#cross-cutting-standards); the [lint bans](#lint-configuration) are not silenced with crate-wide allows.

## Test tiers per crate

The tiers are defined in [PROJECT.md, Testing Strategy](.project/PROJECT.md#testing-strategy) ([D-003](.project/DECISIONS.md#d-003-fixture-first-multi-tier-testing)). This table is that strategy's "Applies to" column, turned around per crate:

| Crate | Tiers that apply | Notes |
|-------|------------------|-------|
| `rr-core` | T0 unit, T1 property, T2 known-answer, T3 fuzz, T7 mutation, T8 coverage | Mutation-gated: `encoding`, `expr`. Coverage ≥ 90%. |
| `rr-defs` | T0, T2, T3, T8 | Every reader and parser is fuzzed. |
| `rr-rom` | T0, T1, T2, T6 safety, T7, T8 | Mutation-gated: journal, checksum, table write path. Coverage ≥ 90%. |
| `rr-link` | T0, T3, T8, T9 hardware-in-loop | HIL is skipped unless `RR_HIL_PORT` / `RR_HIL_J2534` is set. |
| `rr-comms` | T0, T1, T2, T3, T4 replay, T5 simulator, T6, T7, T8, T9 | Mutation-gated: write gating. Coverage ≥ 90%. |
| `rr-live` | T0, T2, T4, T5, T8 | |
| `rr-cli` | T0, T8 | Also the main integration-test surface (ARCHITECTURE.md [Crate Map](.project/ARCHITECTURE.md#crate-map)). |
| `rr-app` | T0, T6, T8, T10 UI smoke | |
| `rr-testkit` | T0, T8 | Dev-dependency only. Hosts the T2(b) reference encoder, which shares no code with `rr-core`. |

ARCHITECTURE.md [Testing](.project/ARCHITECTURE.md#testing) adds that every crate has unit and property tests, and every reader and parser is fuzzed.

**What CI runs today:** `ci.yml` runs T0 (nextest plus doctests) on all four targets, and the T6 safety-invariant suite (the `t6` job). The coverage, mutation, fuzz and fixture jobs described in [INFRASTRUCTURE.md, Build Pipeline](.project/INFRASTRUCTURE.md#build-pipeline) are not wired yet. Until they are, run the relevant tools locally (see [Development Setup](.project/PROJECT.md#development-setup)) when your change touches a tier they cover.

**Shared test tooling lives in `rr-testkit`:** `snapshot` (preconfigured `insta` settings and redactions for paths, wall times and durations), `strategies` (proptest strategies per concept: capability, clock, encoding, expr, layered), plus `fixtures` and `clock` (placeholders, being filled by US-RR-6). Use these rather than writing new helpers. Timing tests use `VirtualClock`, never real time ([D-010](.project/DECISIONS.md#d-010-injected-clock-ban-direct-sleep-and-instantnow)).

## Ledger rules (Must-Fix and Edge Cases)

PROJECT.md keeps two ledgers:

- [Must-Fix Bugs](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase) (`MF-xx`): Java behaviours the Rust version must not reproduce.
- [Edge Cases](.project/PROJECT.md#edge-cases-must-have-tests) (`EC-xx`): inputs and situations that must have tests.

Rules ([PROJECT.md Testing Strategy](.project/PROJECT.md#testing-strategy), [D-003](.project/DECISIONS.md#d-003-fixture-first-multi-tier-testing)):

- **An entry closes only with a named, passing test.** The PR names the test (crate, path and test function) for each `MF-xx` / `EC-xx` it closes. "Covered somewhere" is not enough.
- Each Edge Case needs at least one test, in the tier shown in brackets, before its area's milestone closes.
- A deliberate departure from legacy behaviour is recorded in the Must-Fix ledger.
- A bug found on a real car adds a trace (T4). A fuzz crash becomes a regression test (T3).
- After an ECU-safety incident, the fix adds a Must-Fix entry, and a DECISIONS entry if the design was at fault ([SECURITY.md, Incident Response](.project/SECURITY.md#incident-response)).

## Concept catalogue rule

From [VISION.md Principle 3](.project/VISION.md#principles) and [D-002](.project/DECISIONS.md#d-002-unified-concept-catalogue-minimise-distinct-structures):

- Before adding a struct, enum or trait that crosses a crate boundary, check the [Concept Catalogue](.project/ARCHITECTURE.md#concept-catalogue) (C1–C16). If a concept fits, use it.
- A new top-level concept needs a [DECISIONS.md](.project/DECISIONS.md) entry explaining why no existing one fits, **plus** a new catalogue row. Reviews check this.
- Quirks are data, not code: timing, framing, retries and per-ECU oddities go in layered profiles as Rules (C8) with an `id`, a `note` and a `source`. Protocol and table code never special-cases a specific ECU ([D-004](.project/DECISIONS.md#d-004-timing-framing-and-quirks-are-layered-data-not-code)).
- Respect the dependency rule in the [Crate Map](.project/ARCHITECTURE.md#crate-map). `cargo xtask check-deps` enforces it: `rr-core` depends on no workspace crate, only `rr-link` / `rr-cli` / `rr-app` reach OS hardware crates, and `rr-testkit` is a dev-dependency only.

## Fixture source rule

From [PROJECT.md Testing Strategy](.project/PROJECT.md#testing-strategy), ARCHITECTURE.md [Contracts](.project/ARCHITECTURE.md#contracts), [D-017](.project/DECISIONS.md#d-017-no-proprietary-roms-in-the-public-repo), [D-018](.project/DECISIONS.md#d-018-trace-format-is-the-fixture-format) and [D-025](.project/DECISIONS.md#d-025-external-stock-rom-corpus-fetched-and-pinned-never-vendored):

- **Every expected value states its source:** hand-calculated, standard document, stock ROM, car trace, or cross-source agreement.
- **Changing an expected value is a reviewed PR** that explains why the old value was wrong. Applies to `insta` snapshots too: review them with `cargo insta review` and say why they changed.
- **No proprietary ROMs in the repo.** Public fixtures are synthetic (from the reference encoder in `rr-testkit`) or scrubbed. Real ROMs and car traces live in the private corpus (`RR_FIXTURES_DIR`). The external stock-ROM corpus is fetched by `cargo xtask fetch-corpus` into `RR_CORPUS_DIR` and verified against a committed manifest; the images themselves are never committed.
- Traces (`.rrtrace`) are the fixture format for comms. Scrub VINs and serials before committing or attaching one ([SECURITY.md, PII Handling](.project/SECURITY.md#pii-handling)).
- Fixture files declare `schema = N`.

Not built yet: the `fixtures/` directory, `cargo xtask fetch-corpus`, and the ROM-leak guard that rejects binaries over 64 KB or known ROM IDs under `fixtures/public`. Follow the rules anyway. The fixture loader is described in [Fixtures and corpora](#fixtures-and-corpora).

## Fixtures and corpora

Tests read fixtures through `rr_testkit::fixtures` (full rules in the module docs of `crates/rr-testkit/src/fixtures.rs`). There are three tiers:

| Tier | Where | What turns it on |
|------|-------|------------------|
| Public | `fixtures/` in this repo (synthetic or scrubbed) | Always on. |
| External corpus | `RR_CORPUS_DIR`: stock ROMs and definitions fetched by `cargo xtask fetch-corpus` (its default download location is `~/.cache/romraider-rs/corpus`) | Set `RR_CORPUS_DIR`. The default location alone does not turn the tier on. |
| Private | `RR_FIXTURES_DIR`: real ROMs and car traces, never in the public repo | Build with `--features private-fixtures` **and** set `RR_FIXTURES_DIR`. |

Writing such tests:

- Wrap tests that need the corpus in `rr_testkit::corpus_test! { fn name() { .. } }`, and tests that need private fixtures in `rr_testkit::private_test! { .. }`. Get the root with `fixture_root(Tier::Corpus)` or `fixture_root(Tier::Private)`.
- A crate with private-tier tests forwards the feature to its `rr-testkit` dev-dependency: `[features] private-fixtures = ["rr-testkit/private-fixtures"]` (see `crates/rr-rom/Cargo.toml`). The feature only changes test builds. The corpus tier has no feature, because the corpus is public data.
- Fixture files declare `schema = N`. `load_kat` rejects a missing or unsupported schema and names the file.

Running them:

```sh
cargo nextest run --workspace                                   # public only; corpus and private tests are skipped
RR_CORPUS_DIR=~/.cache/romraider-rs/corpus cargo nextest run --workspace
RR_FIXTURES_DIR=~/rr-fixtures cargo nextest run --workspace --features private-fixtures
```

`--workspace --features private-fixtures` turns the feature on for every crate that declares it. Cargo errors only if none of the selected crates has it. Changing either variable or the feature rebuilds the affected tests.

**A missing tier is a skip, never a pass.** Without its tier, a test is compiled as `#[ignore]`d. The test name says what to set, because nextest does not print ignore reasons. The summary counts these tests as `skipped`, and `--status-level skip` lists them:

```text
SKIP [         ] (─────) rr-rom::corpus_tier stock_rom_corpus_is_reachable::requires_RR_CORPUS_DIR
SKIP [         ] (─────) rr-rom::corpus_tier private_corpus_is_reachable::requires_feature_private_fixtures_and_RR_FIXTURES_DIR
Summary [   0.030s] 35 tests run: 35 passed, 4 skipped
```

`cargo test` shows the reason instead, for example ``ignored, private fixtures disabled: build with `--features private-fixtures` and set RR_FIXTURES_DIR to run``. Forcing skipped tests to run (`--run-ignored all`) without their tier makes them **fail**, with a message naming the feature or variable. A variable that is set but does not name a directory also fails the test.

The public CI jobs (`test`, `t6`, `clippy`, `snapshots`) set neither variable and do not enable `private-fixtures`, so these tests are always skipped there. Before a PR that touches code with private-tier tests, run them locally.

## No-Java rule (D-024)

From [D-024](.project/DECISIONS.md#d-024-no-java-in-the-project-including-tests-and-tooling):

- No Java code is written, built or run by the project: no recorder, no Java CI, no JDK prerequisite.
- `legacy/` is **read-only reference source**. Read it to understand behaviour and file formats; don't modify it (upstream merges are the exception, see [INFRASTRUCTURE.md, Hosting & Services](.project/INFRASTRUCTURE.md#hosting--services)).
- "Matches Java" is not a test oracle. Use the Java-free oracles listed in T2.
- You may run the Java app by hand in a temp directory to compare outputs. Nothing from that is committed or gated on.

## Safety review process

The project is read-only by default; writes are capability-gated ([VISION.md Principle 1](.project/VISION.md#principles), [D-013](.project/DECISIONS.md#d-013-read-only-by-default-capability-gated-writes-no-flashing), [SECURITY.md, Authorization](.project/SECURITY.md#authorization)).

**Safety-gated code** is:

- the capability tokens in `rr-core::capability` (`RomWrite`, `EcuMemoryWrite`, `EcuReset`; there is deliberately no `EcuFlash`);
- every write path that consumes a token: `rr-rom` save, journal, checksum and table write path, and `rr-comms` write gating;
- the mutation-gated modules from T7: `rr-core::encoding`, `rr-core::expr`.

**Rules for changes there:**

- Write APIs take a capability token. Tokens are minted only through `rr-core`'s `confirmation-authority` feature, and **only `rr-app` and `rr-cli` may enable that feature.** Don't enable it anywhere else, and don't add a public constructor, `Clone`, `Default` or `Deserialize` to a token.
- The T6 safety-invariant suite must pass. It is release-blocking: the `t6` job in `ci.yml` (`T6 safety invariants`) runs `cargo xtask check-writes` and every T6 test, and is part of `ci-ok`.
- Name T6 tests so CI selects them: put them in a test binary named `t6_*.rs`, or name the test function or module `t6_*`. The job's nextest filter is `binary(/^t6_/) | test(/(^|::)t6_/) | (package(rr-core) & test(/^capability::/))`, and it fails if nothing matches.
- T7: zero surviving mutants in gated modules; justified exclusions go in `mutants.toml`.
- HIL write tests run only on a bench ECU with `RR_HIL_ALLOW_WRITE` set.
- Flag the PR as touching safety-gated code in the PR checklist. It needs a second approving review: [`.github/CODEOWNERS`](.github/CODEOWNERS) owns these paths (and the lint, check and CI files that enforce them), so a code owner other than the author must approve. The PR template checklist is being added by US-RR-8-6. Apply the `safety-gated` label and complete the safety section of the [PR template](.github/pull_request_template.md).

### Branch protection (maintainers)

CODEOWNERS only takes effect with branch protection. GitHub can't set a different approval count per path, so the setup is: 1 required approval, **Require review from Code Owners**, and approval of the most recent push. A PR touching an owned path therefore needs a code owner who is not the author to approve. Other PRs need one approval from anyone. Required check: `ci-ok`. While there is only one code owner, PRs that owner authors on owned paths need an admin bypass (`enforce_admins` is `false`). For a strict two-approval rule on every PR, set `required_approving_review_count` to `2`. CODEOWNERS is read from the PR's base branch, so commit it to each protected branch first.

Apply to `master` and `rust-rewrite` (needs repo admin; re-running replaces the existing protection):

```sh
for branch in master rust-rewrite; do
  gh api --method PUT "repos/Biztactix-Ryan/RomRaider/branches/$branch/protection" \
    -H "Accept: application/vnd.github+json" --input - <<'JSON'
{
  "required_status_checks": { "strict": true, "contexts": ["ci-ok"] },
  "enforce_admins": false,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "required_approving_review_count": 1,
    "require_last_push_approval": true
  },
  "restrictions": null,
  "required_conversation_resolution": true,
  "allow_force_pushes": false,
  "allow_deletions": false
}
JSON
done
```

Check it with `cargo xtask check-branch-protection`. It makes read-only `gh api` calls (network and repo-admin read access needed, so CI doesn't run it). For each branch in the loop above, it fails unless the live protection equals this payload, `.github/CODEOWNERS` on that branch matches the local file, and GitHub reports no CODEOWNERS errors. Offline, `xtask/tests/codeowners.rs` checks in CI that CODEOWNERS owns every gated path and that this payload requires code-owner review.

## Local checks before a PR

Run these from the repo root. They match the `ci.yml` jobs (`fmt`, `clippy`, `test`, `deny`, `t6`, `snapshots`); the aggregate `ci-ok` check needs them all.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
cargo xtask check-deps
cargo xtask check-globals
cargo xtask lint-selftest
cargo xtask fixtures-selftest   # corpus/private-tier tests are SKIPped (never PASS) without RR_CORPUS_DIR / RR_FIXTURES_DIR
cargo xtask check-writes
cargo xtask gates-selftest   # proves the fmt / clippy / nextest commands fail on a planted violation
cargo deny check
cargo xtask deny-selftest   # needs network; proves deny.toml rejects a bad licence / RUSTSEC advisory
cargo insta test --workspace --check --unreferenced=reject --test-runner nextest --disable-nextest-doctest
```

Notes:

- The toolchain is pinned in `rust-toolchain.toml`; rustup picks it up automatically.
- `cargo-nextest`, `cargo-deny` and `cargo-insta` are separate installs (`cargo install cargo-nextest cargo-deny cargo-insta`, or prebuilt binaries). The `snapshots` job also fails if any `*.snap.new` / `*.pending-snap` file is committed, or if a committed `.snap` is no longer referenced by any test.
- CI also runs clippy (`-D warnings`), the tests and `cargo xtask gates-selftest --target <target>` on `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc` with `RUSTFLAGS=-D warnings`, so `cfg(windows)` code that the Linux clippy job never sees still fails on clippy and compiler warnings.
- `cargo deny check` enforces the licence allowlist (GPL-2.0-or-later compatible, [D-016](.project/DECISIONS.md#d-016-licence-gpl-20-or-later)), RustSec advisories, bans and sources from `deny.toml`. A new dependency must pass it.
- Tier-specific tools (coverage, mutants, fuzz, snapshots) are listed in [Development Setup](.project/PROJECT.md#development-setup).

## Lint configuration

These bans enforce ARCHITECTURE.md [Cross-cutting Standards](.project/ARCHITECTURE.md#cross-cutting-standards). They live in four places:

- **Workspace `Cargo.toml`**, `[workspace.lints.rust]` and `[workspace.lints.clippy]`. Every crate inherits them with `[lints] workspace = true`.
- **`clippy.toml`**: the `disallowed-methods` list, with a reason on each entry, and the `allow-{unwrap,expect,panic}-in-tests` switches.
- **`cargo xtask check-globals`** (`xtask/src/globals.rs`, allowlist `xtask/globals-allowlist.toml`): mutable globals. It scans every `.rs` file under `crates/` and `xtask/`, parsing them with `syn`, so text in comments and strings never matches.
- **`cargo xtask check-deps`** (`xtask/src/deps.rs`, table `RULES`) and **`cargo deny check`** (`deny.toml`): the dependency rule and the supply chain.

CI runs clippy with `-D warnings`, so a lint at `warn` level fails the build too. The `clippy` job also runs both xtask checks, and the `deny` job runs cargo-deny.

`cargo xtask lint-selftest` (`xtask/src/lint_selftest.rs`, also in the `clippy` job) proves the bans actually fire: it generates a fixture workspace under `target/lint-selftest/` that inherits the root `[workspace.lints]` and a copy of `clippy.toml`, plants each banned call (in library code, a `#[cfg(test)]` module and an integration-test crate), runs clippy with the CI arguments, and fails unless every planted violation is reported and nothing else is (so `unwrap` in tests must stay allowed). The test `xtask/tests/lint_docs.rs` fails if a lint in `[workspace.lints]`, a `clippy.toml` method or switch has no row (with its level, a reason and an alternative) in the table below.

| Pattern | Enforced by | Why (refs) | Use instead |
|---------|-------------|------------|-------------|
| `.unwrap()` | `clippy::unwrap_used` = deny. Allowed in tests (`allow-unwrap-in-tests`). | Library code never panics on input ([Errors](.project/ARCHITECTURE.md#errors), [D-019](.project/DECISIONS.md#d-019-errors-logging-and-no-ui-in-libraries)). | Return an error with `?`: a `thiserror` enum per crate (`anyhow` only in `rr-cli` / `rr-app`). Recoverable load problems become a **Diagnostic** (C15, `rr-core::diag`): `{ severity, provenance, message, code }`, collected rather than thrown, so loading returns `(T, Vec<Diagnostic>)`. |
| `.expect(..)` | `clippy::expect_used` = deny. Allowed in tests (`allow-expect-in-tests`). | Same as `unwrap` ([D-019](.project/DECISIONS.md#d-019-errors-logging-and-no-ui-in-libraries)). | Same as `unwrap`. |
| `panic!(..)` | `clippy::panic` = deny. Allowed in tests (`allow-panic-in-tests`). Denied in every crate, binaries and `xtask` included, because Cargo cannot mix `[lints] workspace = true` with per-crate entries. | Library code never panics on input ([D-019](.project/DECISIONS.md#d-019-errors-logging-and-no-ui-in-libraries)). | An error value or a Diagnostic (C15), as above. |
| `todo!()`, `unimplemented!()` | `clippy::todo`, `clippy::unimplemented` = warn, which fails CI under `-D warnings`. | Both panic when reached ([D-019](.project/DECISIONS.md#d-019-errors-logging-and-no-ui-in-libraries)). | Finish the code path, or return an error saying it is unsupported. |
| `dbg!(..)` | `clippy::dbg_macro` = deny, in tests too (there is no in-tests switch for it). | Library code never prints; logs go through `tracing` ([Logging](.project/ARCHITECTURE.md#logging), [D-019](.project/DECISIONS.md#d-019-errors-logging-and-no-ui-in-libraries)). | `tracing` macros (`tracing::debug!` and so on), filtered by `RR_LOG`. Comms record a Trace (C16), not hex debug lines. |
| `std::thread::sleep` | `clippy::disallowed_methods` = deny (`clippy.toml`). No in-tests switch, on purpose. | All timing goes through the Clock ([Time](.project/ARCHITECTURE.md#time), [D-010](.project/DECISIONS.md#d-010-injected-clock-ban-direct-sleep-and-instantnow)). Java counted read timeouts in `sleep(1)` iterations, so on Windows the real timeout could be ~15× longer ([MF-27](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)). | `Clock::sleep` / `Clock::sleep_until` on an injected **Clock** (C9, `rr_core::clock`): `RealClock` in production, `VirtualClock` in tests. Deadlines come from `Clock::deadline_after`. |
| `std::time::Instant::now` | `clippy::disallowed_methods` = deny. | Only `RealClock` may read the OS monotonic clock; injected time keeps timing tests deterministic and instant ([D-010](.project/DECISIONS.md#d-010-injected-clock-ban-direct-sleep-and-instantnow), [MF-27](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)). | `Clock::now`. |
| `std::time::Instant::elapsed` | `clippy::disallowed_methods` = deny. | Reads the OS clock implicitly ([D-010](.project/DECISIONS.md#d-010-injected-clock-ban-direct-sleep-and-instantnow)). | `clock.now() - start`. |
| `std::time::SystemTime::now` | `clippy::disallowed_methods` = deny. | Only `RealClock` may read the OS wall clock ([D-010](.project/DECISIONS.md#d-010-injected-clock-ban-direct-sleep-and-instantnow)). | `Clock::wall_now`, for timestamps only. |
| `std::time::SystemTime::elapsed` | `clippy::disallowed_methods` = deny. | Reads the OS clock implicitly, and wall time must not measure intervals: it jumps with NTP and DST ([EC-26](.project/PROJECT.md#edge-cases-must-have-tests), [D-010](.project/DECISIONS.md#d-010-injected-clock-ban-direct-sleep-and-instantnow)). | `Clock::now` (monotonic). |
| `static mut` (any type) | `cargo xtask check-globals`. | No shared mutable state, no `static mut`, no global singletons ([Threading](.project/ARCHITECTURE.md#threading), [D-011](.project/DECISIONS.md#d-011-os-threads--bounded-channels-no-async-runtime-in-the-core)). Java's stop flags were non-volatile, one of them static ([MF-34](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)), and its settings had static fields set through instance setters ([MF-35](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)). | Explicit owned state, passed in: a value the caller owns, such as the app's state struct (the shared AppState of [D-015](.project/DECISIONS.md#d-015-one-process-single-instance-via-local-socket)). Between threads, use bounded `crossbeam-channel`s, and stop threads with cancellation tokens ([D-011](.project/DECISIONS.md#d-011-os-threads--bounded-channels-no-async-runtime-in-the-core)). |
| A `static` whose type mentions `Mutex`, `RwLock`, `Cell`, `RefCell`, `UnsafeCell`, `SyncUnsafeCell`, `Condvar` or `Atomic*`. This covers lazy singletons such as `LazyLock<Mutex<_>>` and `once_cell::sync::Lazy<RwLock<_>>`. | `cargo xtask check-globals`. | A global singleton holding mutable state ([D-011](.project/DECISIONS.md#d-011-os-threads--bounded-channels-no-async-runtime-in-the-core), [MF-34](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase), [MF-35](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)). | Same as `static mut`. Read-only statics (`static NAME: &str`, `static RE: LazyLock<Regex>`) and `const` items are not flagged. |
| `lazy_static! { static ref .. }` holding one of those types | `cargo xtask check-globals`. | Same: a mutable global singleton ([D-011](.project/DECISIONS.md#d-011-os-threads--bounded-channels-no-async-runtime-in-the-core)). | Same as `static mut`. |
| `thread_local! { static .. }` (any type) | `cargo xtask check-globals`. | Per-thread global state is still a global ([D-011](.project/DECISIONS.md#d-011-os-threads--bounded-channels-no-async-runtime-in-the-core)). | Same as `static mut`. |
| Ignoring a `#[must_use]` value, such as a `Result` | `unused_must_use` = deny (rustc). | A dropped `Result` is a swallowed error. Every failure should be visible and testable ([D-019](.project/DECISIONS.md#d-019-errors-logging-and-no-ui-in-libraries)). | Handle it or propagate it with `?`. If discarding it really is right, write `let _ = ..;` with a comment. |
| An unsafe operation inside an `unsafe fn` without its own `unsafe { }` block | `unsafe_op_in_unsafe_fn` = deny (rustc). | Keeps each unsafe operation in an explicit, reviewable block. This matters most in FFI code such as the J2534 DLL Link. No D-entry; workspace `Cargo.toml`. | Wrap each unsafe operation in `unsafe { }` with a `// SAFETY:` comment. |
| Pre-2018 idioms; anything else in `clippy::all` | `rust_2018_idioms`, `clippy::all` = warn, which fails CI under `-D warnings`. | `clippy -D warnings` in CI ([Style](.project/ARCHITECTURE.md#style)). | Follow the lint's suggestion. |
| A workspace dependency in `rr-core`; `serialport`, `libloading`, `nusb` or `winreg` outside `rr-link` / `rr-cli` / `rr-app`; `rr-testkit` as anything but a dev-dependency | `cargo xtask check-deps` (`RULES` in `xtask/src/deps.rs`). | The [Crate Map](.project/ARCHITECTURE.md#crate-map) dependency rule: `rr-core` depends on nothing in the workspace, and only `rr-link`, `rr-cli` and `rr-app` touch the OS, so everything else is testable without hardware. | Put OS and hardware access behind the Link trait (C7) in `rr-link`. `rr-core` may take `rr-testkit` as a dev-dependency only. To change the rule, edit `RULES`, and nowhere else, together with the Crate Map. |
| A dependency with a licence that is not GPL-2.0-or-later compatible, a RustSec advisory (vulnerable, unmaintained, unsound or yanked), a `*` version requirement, or a source other than crates.io | `cargo deny check` (`deny.toml`). | The project is `GPL-2.0-or-later` and the licence allowlist is restricted to compatible licences: MIT, Apache-2.0 (via GPLv3), BSD-2/3-Clause, ISC, Zlib, MPL-2.0, Unicode ([D-016](.project/DECISIONS.md#d-016-licence-gpl-20-or-later)). | Choose a compatible crate from crates.io with a real version requirement. An advisory exception goes in `[advisories] ignore` with its ID and a reason. |

### Exceptions

- **Use a local `#[expect(<lint>, reason = "...")]`, never `#[allow]`.** Put it on the smallest item that needs it, with a `reason` that says why the code cannot fail or why the ban does not apply. `#[expect]` fails the build if the lint ever stops firing, so a stale exception gets noticed. Crate-wide or module-wide allows (`#![allow(..)]`) are never accepted ([Definition of Done](#definition-of-done), item 8).
- **The only time exception is `rr_core::clock::RealClock`.** Its private `std_time` module is the one place that calls the OS clock and `std::thread::sleep`. Each call is wrapped in `#[expect(clippy::disallowed_methods, reason = "RealClock is the sanctioned std-time caller (D-010)")]`. No other shipping code gets this exception. Tests use `VirtualClock`, and the ban has no in-tests switch; the one test-only exception is a test that must measure real time to prove `VirtualClock` never blocks on it, which may carry its own local `#[expect(clippy::disallowed_methods, reason = "...")]` (in a `tests/` or `benches/` file, or under `#[cfg(test)]` / `#[test]`). `#[allow]`, `#[warn]`, module- or crate-wide `#![..]` forms, a missing `reason`, and suppressing `clippy::all` / `clippy::style` are rejected everywhere. `xtask/tests/time_exceptions.rs` (`xtask/src/time_exceptions.rs`) enforces this in CI.
- **Globals go in the allowlist.** A global that `check-globals` flags needs an `[[allow]]` entry in `xtask/globals-allowlist.toml` with `path` (relative to the workspace root), `item` (the static's name) and a non-empty `justification` saying why no owned, passed-in alternative works. An empty justification fails the check, and so does a stale entry whose item no longer exists. There are no entries today.
- **Exceptions are reviewed like code.** Every new `#[expect]`, allowlist entry, `RULES` change or `deny.toml` exception must be called out in the PR description and approved by a reviewer. In [safety-gated code](#safety-review-process), that means the second approving review as well.
