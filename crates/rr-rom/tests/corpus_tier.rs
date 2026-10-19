//! Cross-crate check that rr-testkit's tier macros work from a dependent
//! crate's tests (rr-testkit is a dev-dependency here).
//!
//! Without `RR_CORPUS_DIR` these corpus tests are reported as skipped
//! (`...::requires_RR_CORPUS_DIR`); with it set they run. The private test
//! needs `--features private-fixtures` (forwarded to rr-testkit by this crate's
//! `[features]`) **and** `RR_FIXTURES_DIR`; without either it is reported as
//! skipped (`...::requires_feature_private_fixtures_and_RR_FIXTURES_DIR`).

use rr_testkit::fixtures::{FixtureError, Tier, fixture_root};

rr_testkit::corpus_test! {
    /// Placeholder for the stock-ROM invariants (T2c): the corpus root exists.
    fn stock_rom_corpus_is_reachable() -> Result<(), FixtureError> {
        let root = fixture_root(Tier::Corpus)?;
        assert!(root.is_dir());
        Ok(())
    }
}

rr_testkit::private_test! {
    /// Placeholder for private-ROM tests: the private corpus root exists.
    fn private_corpus_is_reachable() {
        assert!(fixture_root(Tier::Private).is_ok());
    }
}
