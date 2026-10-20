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
7. **All CI gates are green.** The required checks are `ci-ok` and, on `rust-rewrite`, also `coverage-gate`, `mutants-ok` and `fixtures-ok` (see [Branch protection](#branch-protection-maintainers)); run the [local checks](#local-checks-before-a-pr) before pushing.
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

**What CI runs today:** `ci.yml` runs T0 (nextest plus doctests) on all four targets, and the T6 safety-invariant suite (the `t6` job). `coverage.yml` runs T8 on every pull request and every push to `master` / `rust-rewrite`: `cargo llvm-cov nextest` on the change and on its base, then `cargo xtask coverage-gate`, which fails if rr-core, rr-rom or rr-comms is below 90% line coverage or more than 0.25 points below its own coverage on the base, the measured run-to-run noise (rules in `xtask/src/coverage_gate.rs`; both lcov reports are uploaded as artifacts `lcov` and `lcov-base`). `mutants.yml` runs T7: on a pull request touching a gated crate, `cargo mutants --in-diff` on the changed gated code, or on every gated module when the change touches their tests (`cargo xtask mutants-scope`; a weakened test changes no gated code); weekly, every gated module, with the report uploaded as artifact `mutants-report-full`; and `cargo xtask mutants-selftest`, which proves a weakened test leaves a survivor that fails the job. A surviving mutant fails the job. `mutants.yml` and `fixtures.yml` start on every push / pull request; their first job, `scope`, checks whether the change touches the gate's paths, and the gate jobs are skipped when it doesn't. Their last jobs, `mutants-ok` and `fixtures-ok`, always run: they fail if `scope` failed, if a gate job failed or was cancelled, or if one was skipped although the change is in scope, and pass when it is out of scope. The fuzz and remaining fixture jobs described in [INFRASTRUCTURE.md, Build Pipeline](.project/INFRASTRUCTURE.md#build-pipeline) are not wired yet. Until they are, run the relevant tools locally (see [Development Setup](.project/PROJECT.md#development-setup)) when your change touches a tier they cover.

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

How to write, source and review a known-answer fixture is in [fixtures/README.md](fixtures/README.md).

Not built yet: the ROM-leak guard that rejects binaries over 64 KB or known ROM IDs under `fixtures/public`. Follow the rules anyway.

To get the external corpus, run `cargo xtask fetch-corpus` (about 110 MB download, 650 MB on disk). It fetches the pinned upstream commits of the stock ROMs and of the ECU definitions ([D-028](.project/DECISIONS.md#d-028-ecu-definition-corpus-source-td-dsubarudefs-alpha-pinned-logger-defs-from-the-forum-v370-package-privately-mirrored); kept in the cache's `defs/`) into `RR_CORPUS_DIR` (default `~/.cache/romraider-rs/corpus`, which must be outside the repo) and checks every file's size and SHA-256 against `fixtures/corpus-manifest.toml`. The forum logger package `logger_v370.zip` is never downloaded: if you have the hand-downloaded copy, put it at `RR_CORPUS_DIR/defs/logger_v370.zip` or pass `--logger-zip <path>` and it is hash-checked; without it the run says so and carries on. A second run with a complete cache does not use the network. `--verify-only` checks the cache without fetching, and `--source <git URL or path>` fetches from a mirror instead. Details are in `xtask/src/fetch_corpus.rs`. The fixture loader is described in [Fixtures and corpora](#fixtures-and-corpora).

## Fixtures and corpora

Tests read fixtures through `rr_testkit::fixtures` (full rules in the module docs of `crates/rr-testkit/src/fixtures.rs`). There are three tiers:

| Tier | Where | What turns it on |
|------|-------|------------------|
| Public | `fixtures/` in this repo (synthetic or scrubbed) | Always on. |
| External corpus | `RR_CORPUS_DIR`: stock ROMs and definitions fetched by `cargo xtask fetch-corpus` (its default download location is `~/.cache/romraider-rs/corpus`) | Set `RR_CORPUS_DIR`. The default location alone does not turn the tier on. |
| Private | `RR_FIXTURES_DIR`: real ROMs and car traces, never in the public repo | Build with `--features private-fixtures` **and** set `RR_FIXTURES_DIR`. |

Writing such tests:

- Wrap tests that need the corpus in `rr_testkit::corpus_test! { fn name() { .. } }`, and tests that need private fixtures in `rr_testkit::private_test! { .. }`. Get the root with `fixture_root(Tier::Corpus)` or `fixture_root(Tier::Private)`.
- Read the stock-ROM corpus itself with `rr_testkit::corpus::Corpus::open()` (inside `corpus_test!`). It is read-only, and it refuses an `RR_CORPUS_DIR` inside this repository's working tree (or any of its git worktrees), or one containing it, as `fixture_root(Tier::Corpus)` and `cargo xtask fetch-corpus` also do: the check resolves symlinks and `..`, so no spelling of a path gets around it (`crates/rr-testkit/src/corpus/location.rs`, shared with fetch-corpus). A missing or empty directory is `CorpusError::NotFetched`, separate from a broken corpus. Manifest entries come out as `Entry::Clean` or `Entry::Suspect`, so a test has to decide what to do with the suspect images; use `manifest().clean()` to take only the clean ones.
- A crate with private-tier tests forwards the feature to its `rr-testkit` dev-dependency: `[features] private-fixtures = ["rr-testkit/private-fixtures"]` (see `crates/rr-rom/Cargo.toml`). The feature only changes test builds. The corpus tier has no feature, because the corpus is public data.
- Fixture files declare `schema = N`. `load_kat` rejects a missing or unsupported schema and names the file.
- `cargo xtask validate-fixtures` checks every KAT fixture (`fixtures/kat`, `crates/rr-testkit/tests/data/kat`): each expected value has a `source`, a `hand-calc` source's `working` contains `=`, at least two numbers and the expected value itself, and with `--base <git-rev>` an expected value that differs from the base version needs a new `[[changelog]]` entry. The full rule list is in `crates/rr-testkit/src/kat/validate.rs`; the authoring guide is [fixtures/README.md](fixtures/README.md). CI runs it in `.github/workflows/fixtures.yml` on every push and pull request touching `fixtures/`, `crates/rr-testkit/` or `xtask/`, once on every fixture and once with `--base` (the PR's base commit, or the pre-push commit).
- `cargo xtask gen-synthetic` regenerates the synthetic ROMs, their manifests and `SHA256SUMS` in `fixtures/public/synthetic` from the plans in `crates/rr-testkit/src/reference/synthetic.rs`; never edit those files by hand. `cargo xtask gen-synthetic --check` writes nothing and fails on any byte difference from the committed files. The `synthetic` job of `.github/workflows/fixtures.yml` runs it, and `sha256sum -c SHA256SUMS`, on Linux and Windows. Every synthetic ROM starts with the marker `RomRaider-RS SYNTHETIC ROM v1` and its manifest says `synthetic = true`. Details: [fixtures/README.md, Synthetic ROMs](fixtures/README.md#synthetic-roms).
- Formulas from the definition corpus are never committed as text (D-028: the definitions have no licence). `rr_testkit::corpus::formulas::harvest()` reads every formula out of the fetched corpus at test time (`crates/rr-testkit/src/corpus/formulas.rs`); a KAT case names one by the SHA-256 of its text with `formula_ref` ([fixtures/README.md, `[[expr]]` cases](fixtures/README.md#expr-cases)); and `fixtures/corpus-formulas.toml` (metadata only: hashes, counts, locations) is regenerated with `RR_CORPUS_DIR=<dir> cargo run -p rr-testkit --example gen-corpus-formulas` and checked by the corpus-tier test `corpus_formula_metadata_is_current`.

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
- T7: zero surviving mutants in gated modules (`mutants.yml`). The gated module paths and the exclusions, each with a comment saying why it is justified, are in [`.cargo/mutants.toml`](.cargo/mutants.toml).
- HIL write tests run only on a bench ECU with `RR_HIL_ALLOW_WRITE` set.
- Flag the PR as touching safety-gated code in the PR checklist. It needs a second approving review: [`.github/CODEOWNERS`](.github/CODEOWNERS) owns these paths (and the lint, check and CI files that enforce them), so a code owner other than the author must approve. The PR template checklist is being added by US-RR-8-6. Apply the `safety-gated` label and complete the safety section of the [PR template](.github/pull_request_template.md).

### Branch protection (maintainers)

CODEOWNERS only takes effect with branch protection. GitHub can't set a different approval count per path, so the setup is: 1 required approval, **Require review from Code Owners**, and approval of the most recent push. A PR touching an owned path therefore needs a code owner who is not the author to approve. Other PRs need one approval from anyone. Required checks: `ci-ok` on both branches; on `rust-rewrite` also `coverage-gate` (`coverage.yml`), `mutants-ok` (`mutants.yml`) and `fixtures-ok` (`fixtures.yml`). `master` has none of the Rust gates. Each is a job that runs on every pull request: a path-filtered workflow that never starts would leave its check at "Expected" forever, so `mutants.yml` and `fixtures.yml` decide scope in a job and report through an aggregate job that always runs. While there is only one code owner, PRs that owner authors on owned paths need an admin bypass (`enforce_admins` is `false`). For a strict two-approval rule on every PR, set `required_approving_review_count` to `2`. CODEOWNERS is read from the PR's base branch, so commit it to each protected branch first.

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

Then add the checks `rust-rewrite` requires on top of `ci-ok` (the loop above replaces the whole protection, so run this after it every time):

```sh
gh api --method POST "repos/Biztactix-Ryan/RomRaider/branches/rust-rewrite/protection/required_status_checks/contexts" \
  -H "Accept: application/vnd.github+json" --input - <<'JSON'
{ "contexts": ["coverage-gate", "mutants-ok", "fixtures-ok"] }
JSON
```

Check it with `cargo xtask check-branch-protection`. It makes read-only `gh api` calls (network and repo-admin read access needed, so CI doesn't run it). For each branch in the loop above, it fails unless the live protection equals this payload (plus, for `rust-rewrite`, the added contexts), `.github/CODEOWNERS` on that branch matches the local file, and GitHub reports no CODEOWNERS errors. Offline, `xtask/tests/codeowners.rs` checks in CI that CODEOWNERS owns every gated path and that this payload requires code-owner review.

## Local checks before a PR

Run these from the repo root. They match the `ci.yml` jobs (`fmt`, `clippy`, `test`, `deny`, `t6`, `snapshots`); the aggregate `ci-ok` check needs them all.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
cargo xtask check-deps
cargo xtask check-globals
cargo xtask check-reference-isolation   # rr-testkit::reference (and what it reaches) names no rr-core / workspace crate
cargo xtask lint-selftest
cargo xtask fixtures-selftest   # corpus/private-tier tests are SKIPped (never PASS) without RR_CORPUS_DIR / RR_FIXTURES_DIR
cargo xtask check-writes
cargo xtask check-mutants-exclusions   # every exclusion in .cargo/mutants.toml has a justification comment directly above it
cargo xtask diagnostics-doc --check   # docs/diagnostics.md matches the diagnostic code registry (also a nextest test)
cargo xtask gen-synthetic --check   # fixtures/public/synthetic is byte-identical to a fresh generation (run without --check to regenerate)
cargo xtask validate-fixtures   # KAT fixtures: every expected value sourced, hand-calc shows working; add --base origin/master to require a changelog entry for changed expected values
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
- Coverage, as the `coverage.yml` gate measures it (needs `cargo install cargo-llvm-cov` and `rustup component add llvm-tools-preview`): `cargo llvm-cov nextest --workspace --locked --lcov --output-path lcov.info && cargo xtask coverage-gate --lcov lcov.info`. Add `--base-lcov <file>` with a report from your base branch to check for a regression too.
- Mutation testing, as `mutants.yml` runs it (needs `cargo install cargo-mutants --locked`): `cargo mutants` mutates every gated module listed in `.cargo/mutants.toml`; `git diff --no-color --no-ext-diff origin/rust-rewrite...HEAD > pr.diff && cargo xtask mutants-scope --diff pr.diff` says whether CI will run that or `cargo mutants --in-diff pr.diff` on your change. `cargo xtask mutants-selftest` checks the gate itself. An exclusion (`exclude_globs`, `exclude_re` or `skip_calls` in `.cargo/mutants.toml`) needs its own justification comment on the line(s) directly above it, inside the array: no blank line in between, at least six words, no `TODO` / `FIXME` placeholder. A comment trailing the entry or above `exclude_re = [` does not count. `cargo xtask check-mutants-exclusions` (rules in `xtask/src/mutants_exclusions.rs`) enforces this in the `clippy` job.
- Tier-specific tools (coverage, mutants, fuzz, snapshots) are listed in [Development Setup](.project/PROJECT.md#development-setup).

## Lint configuration

These bans enforce ARCHITECTURE.md [Cross-cutting Standards](.project/ARCHITECTURE.md#cross-cutting-standards). They live in four places:

- **Workspace `Cargo.toml`**, `[workspace.lints.rust]` and `[workspace.lints.clippy]`. Every crate inherits them with `[lints] workspace = true`.
- **`clippy.toml`**: the `disallowed-methods` list, with a reason on each entry, and the `allow-{unwrap,expect,panic}-in-tests` switches.
- **`cargo xtask check-globals`** (`xtask/src/globals.rs`, allowlist `xtask/globals-allowlist.toml`): mutable globals. It scans every `.rs` file under `crates/` and `xtask/`, parsing them with `syn`, so text in comments and strings never matches.
- **`cargo xtask check-deps`** (`xtask/src/deps.rs`, table `RULES`) and **`cargo deny check`** (`deny.toml`): the dependency rule and the supply chain.
- **`cargo xtask check-reference-isolation`** (`xtask/src/reference_isolation.rs`, `RULE`): the module-level rule that keeps the reference codec independent (US-RR-10, T2(b)). No file in `rr-testkit::reference`, or in any rr-testkit module it reaches through `crate::` / `super::` paths, may name `rr_core` or another workspace crate, use `include!` / `#[path]` / `extern crate` (other than std/core/alloc), or call an unqualified macro that is neither std nor defined in a checked file. Sources are lexed with `proc-macro2`, so comments and strings never match. Once the module exists `RULE.required` must be `true`, so renaming it fails the check instead of silently disabling it.

CI runs clippy with `-D warnings`, so a lint at `warn` level fails the build too. The `clippy` job also runs these xtask checks, and the `deny` job runs cargo-deny.

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
| In a stateless module (`STATELESS_PATHS` in `xtask/src/globals.rs`: `rr-core`'s `encoding` and `expr`), any `static`, and any mention of a lock, cell, atomic or lazy/once type (`Mutex`, `RwLock`, `Cell`, `RefCell`, `Atomic*`, `atomic`, `Once`, `OnceLock`, `OnceCell`, `LazyLock`, `LazyCell`, `Lazy`, `thread_local`) in a field, local or `use`. | `cargo xtask check-globals`. | Legacy kept the byte order in a static shared by every open ROM ([MF-04](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)) and every parsed formula in one synchronized global cache ([MF-25](.project/PROJECT.md#must-fix-bugs-from-the-java-codebase)). A lock inside a value still leaves it `Send + Sync`, so only the source shows it. | Carry the state in the value; these values are plain data shared across threads by reference. `Barrier` (test coordination) is allowed. |
| Ignoring a `#[must_use]` value, such as a `Result` | `unused_must_use` = deny (rustc). | A dropped `Result` is a swallowed error. Every failure should be visible and testable ([D-019](.project/DECISIONS.md#d-019-errors-logging-and-no-ui-in-libraries)). | Handle it or propagate it with `?`. If discarding it really is right, write `let _ = ..;` with a comment. |
| An unsafe operation inside an `unsafe fn` without its own `unsafe { }` block | `unsafe_op_in_unsafe_fn` = deny (rustc). | Keeps each unsafe operation in an explicit, reviewable block. This matters most in FFI code such as the J2534 DLL Link. No D-entry; workspace `Cargo.toml`. | Wrap each unsafe operation in `unsafe { }` with a `// SAFETY:` comment. |
| Pre-2018 idioms; anything else in `clippy::all` | `rust_2018_idioms`, `clippy::all` = warn, which fails CI under `-D warnings`. | `clippy -D warnings` in CI ([Style](.project/ARCHITECTURE.md#style)). | Follow the lint's suggestion. |
| A workspace dependency in `rr-core`; `serialport`, `libloading`, `nusb` or `winreg` outside `rr-link` / `rr-cli` / `rr-app`; `rr-testkit` as anything but a dev-dependency | `cargo xtask check-deps` (`RULES` in `xtask/src/deps.rs`). | The [Crate Map](.project/ARCHITECTURE.md#crate-map) dependency rule: `rr-core` depends on nothing in the workspace, and only `rr-link`, `rr-cli` and `rr-app` touch the OS, so everything else is testable without hardware. | Put OS and hardware access behind the Link trait (C7) in `rr-link`. `rr-core` may take `rr-testkit` as a dev-dependency only. To change the rule, edit `RULES`, and nowhere else, together with the Crate Map. |
| A dependency with a licence that is not GPL-2.0-or-later compatible, a RustSec advisory (vulnerable, unmaintained, unsound or yanked), a `*` version requirement, or a source other than crates.io | `cargo deny check` (`deny.toml`). | The project is `GPL-2.0-or-later` and the licence allowlist is restricted to compatible licences: MIT, Apache-2.0 (via GPLv3), BSD-2/3-Clause, ISC, Zlib, MPL-2.0, Unicode ([D-016](.project/DECISIONS.md#d-016-licence-gpl-20-or-later)). | Choose a compatible crate from crates.io with a real version requirement. An advisory exception goes in `[advisories] ignore` with its ID and a reason. |

### Exceptions

- **Use a local `#[expect(<lint>, reason = "...")]`, never `#[allow]`.** Put it on the smallest item that needs it, with a `reason` that says why the code cannot fail or why the ban does not apply. `#[expect]` fails the build if the lint ever stops firing, so a stale exception gets noticed. Crate-wide or module-wide allows (`#![allow(..)]`) are never accepted ([Definition of Done](#definition-of-done), item 8).
- **The only time exception is `rr_core::clock::RealClock`.** Its private `std_time` module is the one place that calls the OS clock and `std::thread::sleep`. Each call is wrapped in `#[expect(clippy::disallowed_methods, reason = "RealClock is the sanctioned std-time caller (D-010)")]`. No other shipping code gets this exception. Tests use `VirtualClock`, and the ban has no in-tests switch; the one test-only exception is a test that must measure real time to prove `VirtualClock` never blocks on it, which may carry its own local `#[expect(clippy::disallowed_methods, reason = "...")]` (in a `tests/` or `benches/` file, or under `#[cfg(test)]` / `#[test]`). `#[allow]`, `#[warn]`, module- or crate-wide `#![..]` forms, a missing `reason`, and suppressing `clippy::all` / `clippy::style` are rejected everywhere. `xtask/tests/time_exceptions.rs` (`xtask/src/time_exceptions.rs`) enforces this in CI.
- **Globals go in the allowlist.** A global that `check-globals` flags needs an `[[allow]]` entry in `xtask/globals-allowlist.toml` with `path` (relative to the workspace root), `item` (the static's name) and a non-empty `justification` saying why no owned, passed-in alternative works. An empty justification fails the check, and so does a stale entry whose item no longer exists. The two entries today are test-only: the per-thread (`thread_local!`) allocation and byte counters of the counting allocator in `crates/rr-core/tests/expr_eval_no_alloc.rs`, which proves expression evaluation allocates nothing and that refusing a formula at a parser limit allocates boundedly.
- **Exceptions are reviewed like code.** Every new `#[expect]`, allowlist entry, `RULES` change or `deny.toml` exception must be called out in the PR description and approved by a reviewer. In [safety-gated code](#safety-review-process), that means the second approving review as well.
