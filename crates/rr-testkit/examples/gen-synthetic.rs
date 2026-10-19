//! `gen-synthetic <OUT_DIR>`: writes the output of every published synthetic
//! ROM plan ([`rr_testkit::reference::synthetic::published_plans`]) into
//! `OUT_DIR`, one `<plan>.bin` ROM image and one `<plan>.manifest.toml`
//! manifest per plan, and prints each file name (US-RR-10-7). Run it as
//! `cargo xtask gen-synthetic`, which writes into a scratch directory, adds
//! the `SHA256SUMS` file and installs or (`--check`) compares the result
//! with `fixtures/public/synthetic/`.
//!
//! Exit status: 0 on success, 1 if a plan does not generate, 2 on a usage or
//! write error. `OUT_DIR` is created if missing; files of the same name are
//! overwritten and nothing else in it is touched.
//!
//! An example target rather than a binary: the workspace's only binaries are
//! `romraider-rs`, `romraider-cli` and `xtask` (US-RR-1-4), and rr-testkit is
//! dev-dependency only, so xtask cannot link the generator itself.

use std::path::PathBuf;
use std::process::ExitCode;

use rr_testkit::reference::synthetic::{output_files, published_plans};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [out] = args.as_slice() else {
        eprintln!("gen-synthetic: usage: gen-synthetic <OUT_DIR>");
        return ExitCode::from(2);
    };
    let out = PathBuf::from(out);
    if let Err(error) = std::fs::create_dir_all(&out) {
        eprintln!("gen-synthetic: error: creating {}: {error}", out.display());
        return ExitCode::from(2);
    }
    for plan in published_plans() {
        let files = match output_files(&plan) {
            Ok(files) => files,
            Err(error) => {
                eprintln!("gen-synthetic: error: plan {:?}: {error}", plan.name);
                return ExitCode::from(1);
            }
        };
        for (name, bytes) in files {
            let path = out.join(&name);
            if let Err(error) = std::fs::write(&path, &bytes) {
                eprintln!("gen-synthetic: error: writing {}: {error}", path.display());
                return ExitCode::from(2);
            }
            println!("{name}");
        }
    }
    ExitCode::SUCCESS
}
