//! US-RR-18-4 on the real tree: RealClock is the only production code
//! allowed to call std time APIs.

use std::path::Path;

use xtask::time_exceptions::{self, Context, SANCTIONED_FILE};

/// Proves US-RR-18-4: `clippy::disallowed_methods` (proven to fire by
/// `cargo xtask lint-selftest`) is suppressed in shipping code only inside
/// RealClock's `std_time` module, and test-code exceptions are reasoned
/// local `#[expect]`s. CI runs this in the `test` job.
#[test]
fn only_realclock_opts_out_of_the_time_ban() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let report = time_exceptions::run(&root).unwrap();
    assert!(report.files > 0, "no sources scanned");
    let v: Vec<String> = report
        .violations()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        v.is_empty(),
        "time-ban exceptions outside RealClock:\n{}",
        v.join("\n")
    );

    // Not vacuous: RealClock's own exceptions are found, and they are the
    // only ones in production code.
    let real: Vec<_> = report
        .exceptions
        .iter()
        .filter(|e| e.context != Context::Test)
        .collect();
    assert!(
        !real.is_empty(),
        "RealClock's std_time exceptions not found"
    );
    for e in real {
        assert_eq!(e.path, SANCTIONED_FILE, "{e}");
        assert_eq!(e.context, Context::RealClock, "{e}");
    }
}
