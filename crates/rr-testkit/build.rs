//! Decides, at build time, which optional fixture tiers are present.
//!
//! Emits `cfg(rr_corpus)` when `RR_CORPUS_DIR` is set (non-empty), and
//! `cfg(rr_private)` when the `private-fixtures` feature is enabled **and**
//! `RR_FIXTURES_DIR` is set (non-empty). rr-testkit uses these cfgs (plus
//! `cfg(feature = "private-fixtures")`) to choose which definition of
//! `corpus_test!` / `private_test!` it exports: with the tier absent the macros
//! mark tests `#[ignore = "..."]`, so the test runner reports them as skipped
//! rather than passed. See `src/fixtures.rs` for the full rationale.
//!
//! `rerun-if-env-changed` makes Cargo rebuild rr-testkit — and therefore every
//! crate whose tests use the macros — whenever either variable changes. A
//! feature change rebuilds the crate (and reruns this script) on its own.

/// `(environment variable, cfg to emit, cargo feature that must also be on)`.
const TIERS: &[(&str, &str, Option<&str>)] = &[
    ("RR_CORPUS_DIR", "rr_corpus", None),
    (
        "RR_FIXTURES_DIR",
        "rr_private",
        Some("CARGO_FEATURE_PRIVATE_FIXTURES"),
    ),
];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    for (env_var, cfg, feature) in TIERS {
        println!("cargo::rerun-if-env-changed={env_var}");
        println!("cargo::rustc-check-cfg=cfg({cfg})");
        let set = std::env::var_os(env_var).is_some_and(|value| !value.is_empty());
        let enabled = feature.is_none_or(|feature| std::env::var_os(feature).is_some());
        if set && enabled {
            println!("cargo::rustc-cfg={cfg}");
        }
    }
}
