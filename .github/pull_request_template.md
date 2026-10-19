<!--
Thanks for contributing to RomRaider-RS. This checklist mirrors the Definition of Done in CONTRIBUTING.md:
https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#definition-of-done
A PR that leaves an applicable item unticked is not ready to merge. Strike through (~~like this~~) or mark "n/a" for items that don't apply, and say why.
-->

## Summary

<!-- What changes, and why. One or two short paragraphs. -->

## Linked work

<!-- ProjectMan IDs this PR implements or advances, e.g. US-RR-8 (story) / US-RR-8-6 (task). -->

- Story: US-RR-
- Task(s): US-RR-

## Definition of Done

### 1. Test tiers touched

[Test tiers per crate](https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#test-tiers-per-crate). Tick each tier this PR adds or changes tests in. Every tier that applies to a crate you changed must be covered, and a bug fix starts with a failing test, trace or fixture.

- [ ] T0 Unit
- [ ] T1 Property
- [ ] T2 Known-answer & invariants
- [ ] T3 Fuzz
- [ ] T4 Replay (`.rrtrace`)
- [ ] T5 Simulator
- [ ] T6 Safety invariants
- [ ] T7 Mutation
- [ ] T8 Coverage (no regression)
- [ ] T9 Hardware-in-loop (read-only unless on a bench ECU with `RR_HIL_ALLOW_WRITE`)
- [ ] T10 UI smoke

Crates changed and the tiers that apply to them:

<!-- e.g. rr-core: T0, T1, T2 added; T3 n/a (no new parser) -->

### 2. Ledger entries closed

[Ledger rules](https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#ledger-rules-must-fix-and-edge-cases). An `MF-xx` / `EC-xx` entry closes only with a named, passing test. "Covered somewhere" is not enough.

- [ ] This PR closes no ledger entries, **or** every entry it closes is listed below with its passing test.

| Entry | Test (crate, path::test_name) |
|-------|-------------------------------|
| <!-- MF-xx / EC-xx --> | <!-- rr-core, crates/rr-core/tests/encoding.rs::int24_round_trip --> |

### 3. Concept catalogue check

[Concept catalogue rule](https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#concept-catalogue-rule).

- [ ] Any new cross-crate struct, enum or trait reuses an existing concept (C1–C16): <!-- name it, e.g. C10 Patch / Journal -->
- [ ] **or** a new top-level concept is added with a DECISIONS.md entry (`D-0xx`) and a new catalogue row: <!-- D-0xx -->
- [ ] Quirks are data (Rules in layered profiles), not ECU-specific code, and `cargo xtask check-deps` passes.

### 4. Fixture sources

[Fixture source rule](https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#fixture-source-rule).

- [ ] Every new or changed expected value states its source (hand-calculated, standard document, stock ROM, car trace, or cross-source agreement).
- [ ] Changed expected values / `insta` snapshots are explained below (why the old value was wrong).
- [ ] No proprietary ROMs committed; any trace or log added is synthetic or scrubbed (`romraider-cli trace scrub` / `romraider-cli log scrub`).

Sources:

<!-- e.g. "MAF scaling KAT: hand-calculated from the definition's expr" -->

### 5. No Java

[No-Java rule](https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#no-java-rule-d-024).

- [ ] Nothing in this PR writes, builds or runs Java, and `legacy/` is untouched.

### 6. Safety-gated paths

[Safety review process](https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#safety-review-process). Safety-gated code: capability tokens in `rr-core::capability`, every write path consuming a token (`rr-rom` save / journal / checksum / table write path, `rr-comms` write gating), and the mutation-gated modules `rr-core::encoding` and `rr-core::expr`.

- [ ] This PR touches **no** safety-gated code.
- [ ] **or** it touches safety-gated code, and:
  - [ ] the PR has the `safety-gated` label;
  - [ ] a second approving review has been requested;
  - [ ] the T6 safety-invariant suite passes;
  - [ ] T7 shows zero surviving mutants in gated modules (exclusions justified in `mutants.toml`);
  - [ ] no capability token gained a public constructor, `Clone`, `Default` or `Deserialize`, and `confirmation-authority` is enabled only by `rr-app` / `rr-cli`.

### 7. CI and standards

[Local checks before a PR](https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#local-checks-before-a-pr).

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace`, `cargo test --workspace --doc`, `cargo xtask check-deps` and `cargo deny check` pass locally; `ci-ok` is green.
- [ ] Errors, logging, threading and time follow the Cross-cutting Standards; [lint bans](https://github.com/Biztactix-Ryan/RomRaider/blob/master/CONTRIBUTING.md#lint-configuration) are not silenced with crate-wide allows.

## Notes for reviewers

<!-- Anything risky, follow-ups, or items marked n/a and why. -->
