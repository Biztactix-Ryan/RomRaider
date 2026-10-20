//! Three-tier fixture loader (ARCHITECTURE.md "Fixture format", D-025).
//!
//! # Tiers
//!
//! | [`Tier`] | Root | Present when |
//! |----------|------|--------------|
//! | [`Tier::Public`] | `<repo>/fixtures` (in-repo synthetic fixtures; `fixtures/kat`, `fixtures/public`, ...) | always |
//! | [`Tier::Corpus`] | `$RR_CORPUS_DIR` (external stock-ROM / definition corpus, fetched by `cargo xtask fetch-corpus`) | `RR_CORPUS_DIR` is set and non-empty |
//! | [`Tier::Private`] | `$RR_FIXTURES_DIR` (Ryan's ROMs and car traces, never in the public repo) | the `private-fixtures` cargo feature is on **and** `RR_FIXTURES_DIR` is set and non-empty |
//!
//! The corpus default location `~/.cache/romraider-rs/corpus`
//! (INFRASTRUCTURE.md) is where `cargo xtask fetch-corpus` *downloads to*; it
//! does **not** by itself make the tier present. Tier presence is decided only
//! by the environment variable, because it must be known at build time (see
//! below) and Cargo cannot cheaply track a directory that may not exist yet.
//! After fetching, run with `RR_CORPUS_DIR=~/.cache/romraider-rs/corpus`.
//!
//! `RR_CORPUS_DIR` must name a directory outside the repository working
//! tree (D-025): [`fixture_root`]`(Tier::Corpus)` returns
//! [`FixtureError::InsideRepo`] otherwise, so every
//! [`corpus_test!`](crate::corpus_test) fails rather than read ROM bytes from
//! inside the repo. [`crate::corpus::Corpus`] is the loader for the corpus
//! itself.
//!
//! A variable that is set but names something that is not a directory is a
//! misconfiguration: [`fixture_root`] returns
//! [`FixtureError::TierNotADirectory`] and the test **fails** (it is not
//! skipped — the user asked for that tier).
//!
//! # Skip semantics (why `#[ignore]` chosen at build time)
//!
//! Rust's test harness has no runtime "skip": a test that returns early is
//! reported as PASSED, which would make a CI run without the corpus look like
//! the corpus tests succeeded. So a missing tier must turn the test into an
//! *ignored* test, which both libtest (`ignored, <reason>`) and nextest
//! (`SKIP`, counted under "skipped") report as not run.
//!
//! `#[ignore]` is static, so the decision is made at build time:
//!
//! 1. rr-testkit's `build.rs` emits `cfg(rr_corpus)` / `cfg(rr_private)` when
//!    `RR_CORPUS_DIR` / `RR_FIXTURES_DIR` is set (`rr_private` also needs the
//!    `private-fixtures` feature, see below), with `rerun-if-env-changed` for
//!    both (and `rustc-check-cfg`, so `unexpected_cfgs` stays quiet).
//! 2. Build-script cfgs do not propagate to dependent crates, so rr-testkit
//!    instead exports **one definition per state** of each test macro
//!    ([`corpus_test!`](crate::corpus_test): present / absent;
//!    [`private_test!`](crate::private_test): present / absent / feature
//!    off). The definition is chosen when rr-testkit is compiled; the
//!    expansion happens in the dependent crate's tests. When the variable or
//!    the feature changes, rr-testkit is rebuilt, and so is every crate whose
//!    tests use the macros — no build script is needed in dependent crates.
//! 3. With the tier absent, the macro adds
//!    `#[ignore = "... set RR_CORPUS_DIR ..."]`; with it present, it emits a
//!    plain `#[test]`.
//! 4. nextest's `SKIP` lines do not print `#[ignore]` reasons, so the variable
//!    to set is also put in the test's *name*: `fn stock_roms_verify()`
//!    becomes the test `stock_roms_verify::requires_RR_CORPUS_DIR` (a module
//!    per test, with `use super::*` so the body sees the enclosing items).
//!    nextest then shows
//!    `SKIP ... rr-rom::corpus stock_roms_verify::requires_RR_CORPUS_DIR` and
//!    counts it under "skipped"; `cargo test` shows
//!    `... ignored, external corpus absent: set RR_CORPUS_DIR to run ...`.
//! 5. Belt and braces: every generated test starts with [`require_tier`], so
//!    forcing ignored tests to run (`--run-ignored all`, `--include-ignored`)
//!    without the tier *fails* with the variable to set, rather than passing
//!    vacuously.
//!
//! # Usage from any crate's tests
//!
//! Add `rr-testkit = { path = "../rr-testkit" }` under `[dev-dependencies]`,
//! then:
//!
//! ```ignore
//! use rr_testkit::fixtures::{fixture_root, FixtureError, Tier};
//!
//! rr_testkit::corpus_test! {
//!     /// Every stock ROM in the corpus verifies.
//!     fn stock_roms_verify() -> Result<(), FixtureError> {
//!         let root = fixture_root(Tier::Corpus)?;
//!         // ... walk `root` ...
//!         Ok(())
//!     }
//! }
//! ```
//!
//! `private_test! { ... }` is the same for [`Tier::Private`]. Several `fn`s may
//! go in one invocation; outer attributes (doc comments, `#[cfg]`, ...) are
//! kept. Public-tier tests are ordinary `#[test]`s.
//!
//! # The `private-fixtures` feature
//!
//! The private tier needs **both** the `private-fixtures` cargo feature on
//! rr-testkit and `RR_FIXTURES_DIR`. The corpus tier needs only
//! `RR_CORPUS_DIR` (it is public data, so there is nothing to opt into).
//!
//! | Feature | `RR_FIXTURES_DIR` | `private_test!` tests |
//! |---------|-------------------|-----------------------|
//! | off | any | ignored: "private fixtures disabled: build with `--features private-fixtures` and set RR_FIXTURES_DIR" |
//! | on | unset / empty | ignored: "private fixtures absent: set RR_FIXTURES_DIR" |
//! | on | set | run |
//!
//! In every row the test is named
//! `name::requires_feature_private_fixtures_and_RR_FIXTURES_DIR`, so the one
//! name tells nextest users both things to do and stays stable for filters.
//! With the feature off, [`fixture_root`]`(Tier::Private)` returns
//! [`FixtureError::FeatureDisabled`] even if the variable is set, so a
//! force-run test cannot reach the private corpus by accident.
//!
//! rr-testkit is a dev-dependency, and Cargo lets a crate's own feature enable
//! a feature of a dev-dependency, so each crate with private-tier tests
//! forwards it:
//!
//! ```toml
//! [features]
//! private-fixtures = ["rr-testkit/private-fixtures"]
//! ```
//!
//! Then `RR_FIXTURES_DIR=... cargo nextest run --workspace --features private-fixtures`
//! turns it on for every selected crate that declares it (Cargo only errors
//! when *no* selected crate has the feature). The feature only affects test
//! builds; it never changes a shipping crate's code.
//!
//! # Known-answer files
//!
//! A KAT fixture is a TOML file whose top level declares `schema = N`
//! ([`SUPPORTED_SCHEMA`]). [`load_kat`] rejects a missing, non-integer or
//! unknown/newer schema with an error naming the file, the version found and
//! the version supported, then deserializes the remaining keys into the
//! caller's type. The schema-1 KAT types themselves live in [`crate::kat`].

use std::fmt;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// The KAT fixture schema version this loader reads.
pub const SUPPORTED_SCHEMA: u32 = 1;

/// Environment variable naming the external stock-ROM corpus directory.
pub const CORPUS_ENV: &str = "RR_CORPUS_DIR";

/// Environment variable naming the private fixture corpus directory.
pub const PRIVATE_ENV: &str = "RR_FIXTURES_DIR";

/// Cargo feature (on rr-testkit, forwarded by each crate with private-tier
/// tests) that must be on, alongside [`PRIVATE_ENV`], for the private tier.
pub const PRIVATE_FEATURE: &str = "private-fixtures";

/// Whether this build of rr-testkit has the [`PRIVATE_FEATURE`] feature.
pub const PRIVATE_FEATURE_ENABLED: bool = cfg!(feature = "private-fixtures");

/// One of the three fixture tiers (see the module docs).
///
/// Deserializes from `"public"`, `"corpus"` or `"private"` (used by KAT
/// fixtures that reference a ROM or definition file, see [`crate::kat`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// In-repo fixtures under `<repo>/fixtures`; always present.
    Public,
    /// External stock-ROM / definition corpus at `$RR_CORPUS_DIR`.
    Corpus,
    /// Private corpus at `$RR_FIXTURES_DIR`.
    Private,
}

impl Tier {
    /// The environment variable that locates this tier, or `None` for
    /// [`Tier::Public`].
    #[must_use]
    pub const fn env_var(self) -> Option<&'static str> {
        match self {
            Self::Public => None,
            Self::Corpus => Some(CORPUS_ENV),
            Self::Private => Some(PRIVATE_ENV),
        }
    }

    /// The cargo feature this tier additionally needs, or `None`. Only
    /// [`Tier::Private`] has one ([`PRIVATE_FEATURE`]).
    #[must_use]
    pub const fn cargo_feature(self) -> Option<&'static str> {
        match self {
            Self::Public | Self::Corpus => None,
            Self::Private => Some(PRIVATE_FEATURE),
        }
    }

    /// Whether this tier was present when rr-testkit was built, i.e. whether
    /// [`corpus_test!`](crate::corpus_test) / [`private_test!`](crate::private_test)
    /// run their tests or mark them ignored. For [`Tier::Private`] this needs
    /// both the [`PRIVATE_FEATURE`] feature and [`PRIVATE_ENV`].
    #[must_use]
    pub const fn present_at_build(self) -> bool {
        match self {
            Self::Public => true,
            Self::Corpus => cfg!(rr_corpus),
            Self::Private => cfg!(rr_private),
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Public => "public",
            Self::Corpus => "external corpus",
            Self::Private => "private",
        })
    }
}

/// An optional tier's environment variable is unset (or empty).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{tier} fixtures are absent: set {env_var} to run this test")]
pub struct TierMissing {
    /// The missing tier.
    pub tier: Tier,
    /// The variable to set.
    pub env_var: &'static str,
}

/// Errors from locating or loading fixtures.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    /// The tier is not available; the message names the variable to set.
    #[error(transparent)]
    TierMissing(#[from] TierMissing),
    /// The tier needs a cargo feature this build of rr-testkit lacks
    /// ([`Tier::Private`] without [`PRIVATE_FEATURE`]).
    #[error(
        "{tier} fixtures are disabled in this build: build with `--features {feature}` and set {env_var} to run this test"
    )]
    FeatureDisabled {
        /// The tier.
        tier: Tier,
        /// The cargo feature to enable.
        feature: &'static str,
        /// The variable to set as well.
        env_var: &'static str,
    },
    /// [`Tier::Corpus`]'s variable names a directory inside this
    /// repository's working tree, or one containing it (D-025; see
    /// [`crate::corpus::location`]).
    #[error(transparent)]
    InsideRepo(#[from] crate::corpus::RepoGuardError),
    /// The tier's variable is set but does not name a directory.
    #[error("{tier} fixtures misconfigured: {env_var}={} is not a directory", path.display())]
    TierNotADirectory {
        /// The tier.
        tier: Tier,
        /// The variable that was set.
        env_var: &'static str,
        /// Its value.
        path: PathBuf,
    },
    /// The fixture file could not be read.
    #[error("cannot read fixture {}: {source}", path.display())]
    Io {
        /// The fixture file.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The fixture file is not valid TOML.
    #[error("fixture {} is not valid TOML: {source}", path.display())]
    Toml {
        /// The fixture file.
        path: PathBuf,
        /// The underlying error.
        source: toml::de::Error,
    },
    /// The fixture file has no top-level `schema` key.
    #[error(
        "fixture {} has no top-level `schema` key (supported schema: {supported})",
        path.display()
    )]
    MissingSchema {
        /// The fixture file.
        path: PathBuf,
        /// [`SUPPORTED_SCHEMA`].
        supported: u32,
    },
    /// The fixture's `schema` is not one this loader reads (unknown, newer,
    /// or not an integer).
    #[error(
        "fixture {} has unsupported schema {found} (supported schema: {supported})",
        path.display()
    )]
    UnsupportedSchema {
        /// The fixture file.
        path: PathBuf,
        /// The `schema` value found, as written in TOML.
        found: String,
        /// [`SUPPORTED_SCHEMA`].
        supported: u32,
    },
    /// The fixture's keys do not match the requested type.
    #[error("fixture {} does not match the expected shape: {source}", path.display())]
    Shape {
        /// The fixture file.
        path: PathBuf,
        /// The underlying error.
        source: toml::de::Error,
    },
}

/// A loaded known-answer fixture.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixture<T> {
    /// The file it was read from.
    pub path: PathBuf,
    /// Its declared schema version (always [`SUPPORTED_SCHEMA`] today).
    pub schema: u32,
    /// Every top-level key except `schema`, deserialized.
    pub data: T,
}

/// The workspace root, derived from rr-testkit's own manifest directory
/// (`crates/rr-testkit` → `../..`).
#[must_use]
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The root directory of `tier`.
///
/// [`Tier::Public`] is always `<repo>/fixtures`. The optional tiers read their
/// environment variable at run time.
///
/// # Errors
///
/// [`FixtureError::FeatureDisabled`] for [`Tier::Private`] when rr-testkit was
/// built without [`PRIVATE_FEATURE`] (checked first, whatever the variable
/// says); [`FixtureError::TierMissing`] if the variable is unset or empty;
/// [`FixtureError::TierNotADirectory`] if it does not name a directory.
pub fn fixture_root(tier: Tier) -> Result<PathBuf, FixtureError> {
    let Some(env_var) = tier.env_var() else {
        return Ok(repo_root().join("fixtures"));
    };
    if let Some(feature) = tier.cargo_feature()
        && !PRIVATE_FEATURE_ENABLED
    {
        return Err(FixtureError::FeatureDisabled {
            tier,
            feature,
            env_var,
        });
    }
    let path = match std::env::var_os(env_var) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => return Err(TierMissing { tier, env_var }.into()),
    };
    if tier == Tier::Corpus {
        crate::corpus::location::check_outside_repo(&path, &repo_root())?;
    }
    if path.is_dir() {
        Ok(path)
    } else {
        Err(FixtureError::TierNotADirectory {
            tier,
            env_var,
            path,
        })
    }
}

/// Fails the calling test unless `tier` is available.
///
/// Injected at the top of every [`corpus_test!`](crate::corpus_test) /
/// [`private_test!`](crate::private_test) body so a force-run ignored test can
/// never pass without its fixtures.
///
/// # Panics
///
/// When [`fixture_root`] fails; the message names the variable to set.
#[track_caller]
#[expect(
    clippy::panic,
    reason = "test-only assertion helper: a missing tier must fail the test, never pass it"
)]
pub fn require_tier(tier: Tier) {
    if let Err(error) = fixture_root(tier) {
        panic!("{error}");
    }
}

/// Loads a known-answer TOML fixture, checking its `schema` first.
///
/// # Errors
///
/// [`FixtureError::Io`], [`FixtureError::Toml`],
/// [`FixtureError::MissingSchema`], [`FixtureError::UnsupportedSchema`] or
/// [`FixtureError::Shape`]; each names the file.
pub fn load_kat<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<Fixture<T>, FixtureError> {
    let path = path.as_ref().to_path_buf();
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_kat(path, &text),
        Err(source) => Err(FixtureError::Io { path, source }),
    }
}

/// [`load_kat`] for fixture text already in memory; `path` is used only to
/// name the fixture in errors and in the returned [`Fixture`].
///
/// # Errors
///
/// [`FixtureError::Toml`], [`FixtureError::MissingSchema`],
/// [`FixtureError::UnsupportedSchema`] or [`FixtureError::Shape`]; each names
/// `path`.
pub fn parse_kat<T: DeserializeOwned>(
    path: impl Into<PathBuf>,
    text: &str,
) -> Result<Fixture<T>, FixtureError> {
    let path = path.into();
    let mut table = match toml::from_str::<toml::Table>(text) {
        Ok(table) => table,
        Err(source) => return Err(FixtureError::Toml { path, source }),
    };
    let schema = match table.remove("schema") {
        None => {
            return Err(FixtureError::MissingSchema {
                path,
                supported: SUPPORTED_SCHEMA,
            });
        }
        Some(toml::Value::Integer(n)) if n == i64::from(SUPPORTED_SCHEMA) => SUPPORTED_SCHEMA,
        Some(other) => {
            return Err(FixtureError::UnsupportedSchema {
                path,
                found: other.to_string(),
                supported: SUPPORTED_SCHEMA,
            });
        }
    };
    match toml::Value::Table(table).try_into::<T>() {
        Ok(data) => Ok(Fixture { path, schema, data }),
        Err(source) => Err(FixtureError::Shape { path, source }),
    }
}

/// [`load_kat`] for a path relative to `tier`'s root.
///
/// # Errors
///
/// As [`fixture_root`] and [`load_kat`].
pub fn load_kat_in<T: DeserializeOwned>(
    tier: Tier,
    relative: impl AsRef<Path>,
) -> Result<Fixture<T>, FixtureError> {
    load_kat(fixture_root(tier)?.join(relative))
}

/// Shared expansion of [`corpus_test!`](crate::corpus_test) and
/// [`private_test!`](crate::private_test). Not public API.
///
/// Each `fn name()` becomes `mod name { fn requires_<ENV_VAR>() }`, so the
/// variable to set is part of the test's name and therefore visible in every
/// runner's output (nextest's `SKIP` lines do not print `#[ignore]` reasons).
#[doc(hidden)]
#[macro_export]
macro_rules! __rr_tier_test {
    (tier = $tier:ident, requires = $requires:ident, ignore = [$($ignore:tt)*],) => {};
    (
        tier = $tier:ident,
        requires = $requires:ident,
        ignore = [$($ignore:tt)*],
        $(#[$meta:meta])* fn $name:ident() $(-> $ret:ty)? $body:block
        $($rest:tt)*
    ) => {
        mod $name {
            #[allow(unused_imports, reason = "the test body may use items from the enclosing module")]
            use super::*;

            #[test]
            $($ignore)*
            $(#[$meta])*
            #[allow(non_snake_case, reason = "the test name spells the environment variable to set")]
            fn $requires() $(-> $ret)? {
                $crate::fixtures::require_tier($crate::fixtures::Tier::$tier);
                $body
            }
        }

        $crate::__rr_tier_test! {
            tier = $tier,
            requires = $requires,
            ignore = [$($ignore)*],
            $($rest)*
        }
    };
}

/// Declares tests that need the external stock-ROM corpus ([`Tier::Corpus`]).
///
/// `fn name() { .. }` becomes the test `name::requires_RR_CORPUS_DIR`. This
/// build of rr-testkit had `RR_CORPUS_DIR` set: the tests run normally. See
/// the [`fixtures`](crate::fixtures) module docs for usage and semantics.
#[cfg(rr_corpus)]
#[macro_export]
macro_rules! corpus_test {
    ($($items:tt)*) => {
        $crate::__rr_tier_test! {
            tier = Corpus,
            requires = requires_RR_CORPUS_DIR,
            ignore = [],
            $($items)*
        }
    };
}

/// Declares tests that need the external stock-ROM corpus ([`Tier::Corpus`]).
///
/// `fn name() { .. }` becomes the test `name::requires_RR_CORPUS_DIR`. This
/// build of rr-testkit had no `RR_CORPUS_DIR`: the tests are `#[ignore]`d
/// (reported as skipped). See the [`fixtures`](crate::fixtures) module docs
/// for usage and semantics.
#[cfg(not(rr_corpus))]
#[macro_export]
macro_rules! corpus_test {
    ($($items:tt)*) => {
        $crate::__rr_tier_test! {
            tier = Corpus,
            requires = requires_RR_CORPUS_DIR,
            ignore = [#[ignore = "external corpus absent: set RR_CORPUS_DIR to run (see `cargo xtask fetch-corpus`)"]],
            $($items)*
        }
    };
}

/// Declares tests that need the private fixture corpus ([`Tier::Private`]).
///
/// `fn name() { .. }` becomes the test
/// `name::requires_feature_private_fixtures_and_RR_FIXTURES_DIR`. This build of
/// rr-testkit had the `private-fixtures` feature and `RR_FIXTURES_DIR` set: the
/// tests run normally. See the [`fixtures`](crate::fixtures) module docs for
/// usage and semantics.
#[cfg(rr_private)]
#[macro_export]
macro_rules! private_test {
    ($($items:tt)*) => {
        $crate::__rr_tier_test! {
            tier = Private,
            requires = requires_feature_private_fixtures_and_RR_FIXTURES_DIR,
            ignore = [],
            $($items)*
        }
    };
}

/// Declares tests that need the private fixture corpus ([`Tier::Private`]).
///
/// `fn name() { .. }` becomes the test
/// `name::requires_feature_private_fixtures_and_RR_FIXTURES_DIR`. This build of
/// rr-testkit had the `private-fixtures` feature but no `RR_FIXTURES_DIR`: the
/// tests are `#[ignore]`d (reported as skipped). See the
/// [`fixtures`](crate::fixtures) module docs for usage and semantics.
#[cfg(all(feature = "private-fixtures", not(rr_private)))]
#[macro_export]
macro_rules! private_test {
    ($($items:tt)*) => {
        $crate::__rr_tier_test! {
            tier = Private,
            requires = requires_feature_private_fixtures_and_RR_FIXTURES_DIR,
            ignore = [#[ignore = "private fixtures absent: set RR_FIXTURES_DIR to run"]],
            $($items)*
        }
    };
}

/// Declares tests that need the private fixture corpus ([`Tier::Private`]).
///
/// `fn name() { .. }` becomes the test
/// `name::requires_feature_private_fixtures_and_RR_FIXTURES_DIR`. This build of
/// rr-testkit lacked the `private-fixtures` feature: the tests are
/// `#[ignore]`d (reported as skipped) whatever `RR_FIXTURES_DIR` says. See the
/// [`fixtures`](crate::fixtures) module docs for usage and semantics.
#[cfg(not(feature = "private-fixtures"))]
#[macro_export]
macro_rules! private_test {
    ($($items:tt)*) => {
        $crate::__rr_tier_test! {
            tier = Private,
            requires = requires_feature_private_fixtures_and_RR_FIXTURES_DIR,
            ignore = [#[ignore = "private fixtures disabled: build with `--features private-fixtures` and set RR_FIXTURES_DIR to run"]],
            $($items)*
        }
    };
}
