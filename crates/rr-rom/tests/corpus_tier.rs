//! Cross-crate check that rr-testkit's tier macros work from a dependent
//! crate's tests (rr-testkit is a dev-dependency here).
//!
//! Without `RR_CORPUS_DIR` these corpus tests are reported as skipped
//! (`...::requires_RR_CORPUS_DIR`); with it set they run. The private test
//! needs `--features private-fixtures` (forwarded to rr-testkit by this crate's
//! `[features]`) **and** `RR_FIXTURES_DIR`; without either it is reported as
//! skipped (`...::requires_feature_private_fixtures_and_RR_FIXTURES_DIR`).

use rr_testkit::corpus::{Corpus, CorpusError, Image, STANDARD_SIZES, SuspectKind};
use rr_testkit::fixtures::{FixtureError, Tier, fixture_root};

rr_testkit::corpus_test! {
    /// Placeholder for the stock-ROM invariants (T2c): the corpus root exists.
    fn stock_rom_corpus_is_reachable() -> Result<(), FixtureError> {
        let root = fixture_root(Tier::Corpus)?;
        assert!(root.is_dir());
        Ok(())
    }
}

rr_testkit::corpus_test! {
    /// US-RR-11-2: the shape every stock-ROM consumer here takes. Each entry
    /// is read as an [`Image`] and handled by kind: a raw image is used as
    /// is; a container or trailing-data suspect yields the standard image
    /// embedded in it; any other kind has no image to use and fails this
    /// test until a consumer decides what to do with it. Every one of the 501
    /// entries resolves to a standard-size image, two of them via a
    /// container.
    fn every_entry_is_handled_raw_or_suspect() -> Result<(), CorpusError> {
        let corpus = Corpus::open()?;
        let (mut raw, mut embedded) = (0, 0);
        for entry in corpus.manifest().entries() {
            let file = entry.file();
            let image = corpus.read(file)?;
            let stock: &[u8] = match &image {
                Image::Raw(bytes) => {
                    raw += 1;
                    bytes
                }
                Image::Suspect { bytes, suspect } => match suspect.kind {
                    SuspectKind::Container | SuspectKind::TrailingData => {
                        embedded += 1;
                        suspect.embedded(bytes).unwrap_or_else(|| {
                            panic!("{}: {:?} without an embedded image", file.path, suspect.kind)
                        })
                    }
                    SuspectKind::Blank
                    | SuspectKind::CrlfExpansion
                    | SuspectKind::Truncated
                    | SuspectKind::Unknown => panic!(
                        "{}: {:?} suspect has no image to use; handle it here: {}",
                        file.path, suspect.kind, suspect.reason
                    ),
                },
            };
            assert!(STANDARD_SIZES.contains(&(stock.len() as u64)), "{}", file.path);
        }
        assert_eq!((raw, embedded), (499, 2));
        Ok(())
    }
}

rr_testkit::private_test! {
    /// Placeholder for private-ROM tests: the private corpus root exists.
    fn private_corpus_is_reachable() {
        assert!(fixture_root(Tier::Private).is_ok());
    }
}
