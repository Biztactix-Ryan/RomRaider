//! `gen-corpus-formulas [--check]` (US-RR-20-10, D-028): harvests every
//! formula of the pinned definition corpus at `RR_CORPUS_DIR`
//! ([`rr_testkit::corpus::formulas::harvest`]) and writes the metadata,
//! never the formula text, to `fixtures/corpus-formulas.toml`. With
//! `--check` nothing is written and the exit status says whether the
//! committed file is current; the corpus-tier test
//! `corpus_formula_metadata_is_current::requires_RR_CORPUS_DIR` checks the
//! same thing in CI.
//!
//! ```sh
//! RR_CORPUS_DIR=~/.cache/romraider-rs/corpus cargo run -p rr-testkit --example gen-corpus-formulas
//! ```
//!
//! Exit status: 0 on success (or a current file with `--check`), 1 when
//! `--check` finds the file out of date, 2 on a usage, corpus or write error.
//!
//! An example target rather than a binary: the workspace's only binaries are
//! `romraider-rs`, `romraider-cli` and `xtask` (US-RR-1-4), and rr-testkit is
//! dev-dependency only, so xtask cannot link the harvester itself.

use std::process::ExitCode;

use rr_testkit::corpus::formulas::{self, METADATA_PATH};
use rr_testkit::fixtures::repo_root;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = match args.as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => {
            eprintln!("gen-corpus-formulas: usage: gen-corpus-formulas [--check]");
            return ExitCode::from(2);
        }
    };
    let harvest = match formulas::harvest() {
        Ok(harvest) => harvest,
        Err(error) => {
            eprintln!("gen-corpus-formulas: error: {error}");
            return ExitCode::from(2);
        }
    };
    let rendered = harvest.render_metadata();
    let path = repo_root().join(METADATA_PATH);
    let summary = format!(
        "{} distinct formula(s) in {} occurrence(s) across {} file(s)",
        harvest.formulas.len(),
        harvest.occurrences(),
        harvest.files()
    );
    let current = std::fs::read_to_string(&path).ok();
    if check {
        if current.as_deref() == Some(rendered.as_str()) {
            println!("gen-corpus-formulas: ok: {METADATA_PATH} is current ({summary})");
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "gen-corpus-formulas: {METADATA_PATH} is out of date ({summary}); \
             regenerate it with `cargo run -p rr-testkit --example gen-corpus-formulas`"
        );
        return ExitCode::from(1);
    }
    if current.as_deref() != Some(rendered.as_str())
        && let Err(error) = std::fs::write(&path, &rendered)
    {
        eprintln!(
            "gen-corpus-formulas: error: writing {}: {error}",
            path.display()
        );
        return ExitCode::from(2);
    }
    println!("gen-corpus-formulas: wrote {METADATA_PATH} ({summary})");
    ExitCode::SUCCESS
}
