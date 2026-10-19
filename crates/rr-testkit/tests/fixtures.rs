//! Tests for the three-tier fixture loader (`rr_testkit::fixtures`).
//!
//! US-RR-6-2 ("The fixture loader rejects a fixture file with an unknown
//! schema version") is proven by the `rejects_*_schema` tests below: each
//! loads a fixture from `tests/data/` whose `schema` is newer (2), older (0),
//! not an integer, or missing, and asserts the exact error variant and that it
//! names the file, the version found and the version supported.

use std::path::PathBuf;

use rr_testkit::fixtures::{
    CORPUS_ENV, Fixture, FixtureError, PRIVATE_ENV, PRIVATE_FEATURE, PRIVATE_FEATURE_ENABLED,
    SUPPORTED_SCHEMA, Tier, TierMissing, fixture_root, load_kat, repo_root,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Kat {
    source: String,
    #[serde(default)]
    case: Vec<Case>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Case {
    raw: u8,
    real: f64,
}

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

#[test]
fn loads_supported_schema_without_leaking_schema_key() {
    let fixture: Fixture<Kat> = load_kat(data("kat_v1.toml")).unwrap();
    assert_eq!(fixture.schema, SUPPORTED_SCHEMA);
    assert_eq!(fixture.path, data("kat_v1.toml"));
    assert_eq!(fixture.data.source, "hand-calculated");
    assert_eq!(
        fixture.data.case,
        vec![
            Case {
                raw: 0x7F,
                real: 127.0
            },
            Case { raw: 0, real: 0.0 }
        ]
    );
}

/// Asserts `error` is [`FixtureError::UnsupportedSchema`] for `file`, with
/// `found` as written in TOML and the supported version, and that its message
/// says all three.
fn assert_unsupported_schema(error: &FixtureError, file: &str, expected_found: &str) {
    assert!(
        matches!(
            error,
            FixtureError::UnsupportedSchema { path, found, supported }
                if path == &data(file) && found == expected_found && *supported == SUPPORTED_SCHEMA
        ),
        "expected UnsupportedSchema {{ {file}, found {expected_found}, supported {SUPPORTED_SCHEMA} }}, got {error:?}"
    );
    let message = error.to_string();
    assert!(message.contains(file), "{message}");
    assert!(
        message.contains(&format!("unsupported schema {expected_found}")),
        "{message}"
    );
    assert!(
        message.contains(&format!("supported schema: {SUPPORTED_SCHEMA}")),
        "{message}"
    );
}

/// US-RR-6-2: a newer schema (2) is rejected, naming file, found, supported.
#[test]
fn rejects_newer_schema_naming_file_found_and_supported() {
    let error = load_kat::<Kat>(data("kat_v2.toml")).unwrap_err();
    assert_unsupported_schema(&error, "kat_v2.toml", "2");
}

/// US-RR-6-2: an unknown older schema (0) is rejected the same way.
#[test]
fn rejects_unknown_older_schema() {
    let error = load_kat::<Kat>(data("kat_v0.toml")).unwrap_err();
    assert_unsupported_schema(&error, "kat_v0.toml", "0");
}

/// US-RR-6-2: a schema that is not an integer (`"1"`) is not version 1.
#[test]
fn rejects_non_integer_schema() {
    let error = load_kat::<Kat>(data("kat_schema_string.toml")).unwrap_err();
    assert_unsupported_schema(&error, "kat_schema_string.toml", "\"1\"");
}

/// US-RR-6-2: a fixture with no `schema` at all is rejected, naming the file
/// and the supported version.
#[test]
fn rejects_missing_schema() {
    let error = load_kat::<Kat>(data("kat_no_schema.toml")).unwrap_err();
    assert!(
        matches!(&error, FixtureError::MissingSchema { path, .. } if path == &data("kat_no_schema.toml")),
        "{error:?}"
    );
    assert!(
        matches!(&error, FixtureError::MissingSchema { supported, .. } if *supported == SUPPORTED_SCHEMA),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("kat_no_schema.toml"), "{message}");
    assert!(
        message.contains(&format!("supported schema: {SUPPORTED_SCHEMA}")),
        "{message}"
    );
}

#[test]
fn reports_invalid_toml_and_missing_file_with_path() {
    let error = load_kat::<Kat>(data("kat_bad_toml.toml")).unwrap_err();
    assert!(matches!(error, FixtureError::Toml { .. }), "{error:?}");
    assert!(error.to_string().contains("kat_bad_toml.toml"));

    let error = load_kat::<Kat>(data("does_not_exist.toml")).unwrap_err();
    assert!(matches!(error, FixtureError::Io { .. }), "{error:?}");
    assert!(error.to_string().contains("does_not_exist.toml"));
}

#[test]
fn reports_shape_mismatch() {
    #[derive(Debug, Deserialize)]
    struct Wrong {
        #[expect(dead_code, reason = "only the deserialization failure matters")]
        missing_field: u32,
    }
    let error = load_kat::<Wrong>(data("kat_v1.toml")).unwrap_err();
    assert!(matches!(error, FixtureError::Shape { .. }), "{error:?}");
}

#[test]
fn public_tier_is_repo_fixtures() {
    let root = fixture_root(Tier::Public).unwrap();
    assert_eq!(root, repo_root().join("fixtures"));
    assert!(repo_root().join("Cargo.toml").is_file());
    assert!(Tier::Public.present_at_build());
    assert_eq!(Tier::Public.env_var(), None);
}

#[test]
fn optional_tiers_name_their_env_vars() {
    assert_eq!(Tier::Corpus.env_var(), Some(CORPUS_ENV));
    assert_eq!(Tier::Private.env_var(), Some(PRIVATE_ENV));
    assert_eq!(CORPUS_ENV, "RR_CORPUS_DIR");
    assert_eq!(PRIVATE_ENV, "RR_FIXTURES_DIR");
    let missing = TierMissing {
        tier: Tier::Corpus,
        env_var: CORPUS_ENV,
    };
    assert_eq!(
        missing.to_string(),
        "external corpus fixtures are absent: set RR_CORPUS_DIR to run this test"
    );
}

/// The build-time decision (which drives `#[ignore]`) matches the run-time
/// environment and the `private-fixtures` feature, so a tier is never
/// "present" for one and absent for the other.
#[test]
fn build_time_presence_matches_runtime_env() {
    for tier in [Tier::Corpus, Tier::Private] {
        let var = tier.env_var().unwrap();
        let feature_on = tier.cargo_feature().is_none() || PRIVATE_FEATURE_ENABLED;
        let set = std::env::var_os(var).is_some_and(|v| !v.is_empty());
        assert_eq!(tier.present_at_build(), set && feature_on, "{tier} / {var}");
        match fixture_root(tier) {
            Err(FixtureError::FeatureDisabled {
                tier: disabled,
                feature,
                env_var,
            }) => {
                assert!(!feature_on);
                assert_eq!(disabled, Tier::Private);
                assert_eq!(feature, PRIVATE_FEATURE);
                assert_eq!(env_var, var);
            }
            Err(FixtureError::TierMissing(missing)) => {
                assert!(feature_on && !set);
                assert_eq!(missing.env_var, var);
            }
            Ok(root) => assert!(feature_on && set && root.is_dir()),
            Err(FixtureError::TierNotADirectory { env_var, .. }) => {
                assert!(feature_on && set);
                assert_eq!(env_var, var);
            }
            Err(other) => panic!("unexpected {other:?}"),
        }
    }
}

/// Only the private tier is feature-gated, and the disabled-feature error
/// names both the feature and the variable.
#[test]
fn private_tier_feature_gate_is_named() {
    assert_eq!(PRIVATE_FEATURE, "private-fixtures");
    assert_eq!(PRIVATE_FEATURE_ENABLED, cfg!(feature = "private-fixtures"));
    assert_eq!(Tier::Public.cargo_feature(), None);
    assert_eq!(Tier::Corpus.cargo_feature(), None);
    assert_eq!(Tier::Private.cargo_feature(), Some(PRIVATE_FEATURE));
    let disabled = FixtureError::FeatureDisabled {
        tier: Tier::Private,
        feature: PRIVATE_FEATURE,
        env_var: PRIVATE_ENV,
    };
    assert_eq!(
        disabled.to_string(),
        "private fixtures are disabled in this build: build with \
         `--features private-fixtures` and set RR_FIXTURES_DIR to run this test"
    );
}

rr_testkit::corpus_test! {
    /// Skipped (ignored, with `RR_CORPUS_DIR` named) unless the corpus is set.
    ///
    /// Sentinel for `cargo xtask fixtures-selftest` (US-RR-6-1,
    /// `xtask/src/fixtures_selftest.rs`): must stay, and must be reported as
    /// skipped, never passed, when `RR_CORPUS_DIR` is unset.
    fn corpus_tier_root_is_a_directory() -> Result<(), FixtureError> {
        assert!(fixture_root(Tier::Corpus)?.is_dir());
        Ok(())
    }
}

rr_testkit::private_test! {
    /// Skipped (ignored, with `private-fixtures` and `RR_FIXTURES_DIR` named)
    /// unless the feature is on and the private corpus is set.
    ///
    /// Sentinel for `cargo xtask fixtures-selftest` (US-RR-6-1): must stay, and
    /// must be reported as skipped, never passed, when `RR_FIXTURES_DIR` is
    /// unset (with or without `--features private-fixtures`).
    fn private_tier_root_is_a_directory() -> Result<(), FixtureError> {
        assert!(fixture_root(Tier::Private)?.is_dir());
        Ok(())
    }
}
