//! Severity Error diagnostics make the owning load fail closed — US-RR-17-3.
//!
//! Proves the US-RR-17 acceptance criterion *"the caller cannot get a usable
//! `T` without explicitly accepting errors"* from outside `rr-core`, against
//! the public API only:
//!
//! * **Runtime** — with any [`Severity::Error`] diagnostic, every public path
//!   that yields `T` ([`Loaded::value`], [`Loaded::into_result`],
//!   [`Loaded::map`] then either) withholds it; only
//!   [`Loaded::accept_errors`] / [`Rejected::accept_errors`] release it.
//!   Warnings and infos never withhold. A property test checks this for
//!   arbitrary diagnostic lists, and a real loader
//!   ([`legacy::parse_encoding`]) is checked end to end.
//! * **Compile time** — trybuild cases (`tests/ui/fail_closed/*.rs`) show the
//!   value is not reachable any other way: no public field, no
//!   destructuring, no `Deref`, no `From`/`AsRef`/`Borrow`/`IntoIterator`,
//!   and no forging a [`Rejected`]. `tests/ui/fail_closed_pass` is the
//!   positive control (the explicit opt-in compiles).
//!
//! Regenerate the `.stderr` snapshots with `TRYBUILD=overwrite cargo test -p
//! rr-core --test fail_closed_loads` and review every diff.
//!
//! [`legacy::parse_encoding`]: rr_core::encoding::legacy::parse_encoding

use proptest::prelude::*;
use rr_core::diag::{Code, Diagnostic, Diagnostics, Loaded, Provenance, Rejected, Severity, Span};
use rr_core::encoding::legacy::{LegacyAttributes, parse_encoding};

const CODE: Code = Code::new("DEF0001");

fn here() -> Provenance {
    Provenance::new("defs/16BITBASE.xml", Span::line_col(42, 7))
}

fn diag(severity: Severity) -> Diagnostic {
    Diagnostic::new(severity, CODE, format!("{severity} diagnostic"), here())
}

/// Every public route from a `Loaded<T>` to a `T` other than the explicit
/// opt-in, and what it returned. `None` means the route withheld the value.
fn ungated_routes(loaded: &Loaded<u32>) -> [Option<u32>; 4] {
    [
        loaded.value().copied(),
        loaded.clone().into_result().ok().map(|(v, _)| v),
        loaded.clone().map(|v| v + 1).value().map(|v| v - 1),
        loaded
            .clone()
            .map(|v| v + 1)
            .into_result()
            .ok()
            .map(|(v, _)| v - 1),
    ]
}

#[test]
fn error_diagnostic_withholds_value_on_every_ungated_route() {
    let loaded = Loaded::new(7u32, vec![diag(Severity::Error)]);
    assert!(loaded.has_errors());
    assert_eq!(ungated_routes(&loaded), [None; 4]);
    // The diagnostics are never withheld.
    assert_eq!(loaded.diagnostics().len(), 1);
}

#[test]
fn rejected_holds_the_value_until_explicitly_accepted() {
    let diags = vec![diag(Severity::Warning), diag(Severity::Error)];
    let rejected: Rejected<u32> = match Loaded::new(7u32, diags.clone()).into_result() {
        Ok(_) => panic!("an Error diagnostic must reject the load"),
        Err(rejected) => rejected,
    };
    assert_eq!(rejected.error_count(), 1);
    assert_eq!(rejected.diagnostics(), diags.as_slice());
    // It is a real error type the caller can propagate with `?`.
    let as_error: &dyn std::error::Error = &rejected;
    assert!(
        as_error
            .to_string()
            .starts_with("load failed closed with 1 error")
    );
    assert_eq!(rejected.accept_errors(), (7, diags));
}

#[test]
fn accept_errors_is_the_explicit_opt_in() {
    let diags = vec![diag(Severity::Info), diag(Severity::Error)];
    let (value, kept) = Loaded::new(7u32, diags.clone()).accept_errors();
    assert_eq!((value, kept), (7, diags));
}

#[test]
fn collector_finish_fails_closed_on_an_error() {
    let mut diags = Diagnostics::new();
    diags.warning(CODE, "w", here());
    let ok = diags.clone().finish(7u32);
    assert_eq!(ungated_routes(&ok), [Some(7); 4]);

    diags.error(CODE, "e", here());
    let failed = diags.finish(7u32);
    assert_eq!(ungated_routes(&failed), [None; 4]);
}

#[test]
fn nested_error_folded_into_outer_load_fails_it_closed() {
    let inner = Loaded::new(3u32, vec![diag(Severity::Error)]);
    let mut outer = Diagnostics::new();
    let (value, inner_diags) = inner.accept_errors();
    outer.extend(inner_diags);
    let loaded = outer.finish(value * 2);
    assert_eq!(ungated_routes(&loaded), [None; 4]);
}

#[test]
fn real_loader_fails_closed_on_bad_input() {
    let bad = LegacyAttributes {
        storage_type: "int24",
        endian: "sideways",
        mask: None,
        data_layout: None,
    };
    let loaded = parse_encoding(&bad, &here());
    assert!(loaded.has_errors());
    assert!(loaded.diagnostics().iter().all(Diagnostic::is_error));
    assert_eq!(loaded.value(), None);
    assert!(loaded.clone().into_result().is_err());
    // Even explicitly accepted, the loader never invents a default encoding.
    assert_eq!(loaded.accept_errors().0, None);

    let good = LegacyAttributes {
        storage_type: "uint16",
        endian: "big",
        mask: None,
        data_layout: None,
    };
    let loaded = parse_encoding(&good, &here());
    assert!(loaded.value().is_some_and(Option::is_some));
    assert!(loaded.into_result().is_ok());
}

fn severity() -> impl Strategy<Value = Severity> {
    prop_oneof![
        Just(Severity::Info),
        Just(Severity::Warning),
        Just(Severity::Error),
    ]
}

proptest! {
    /// For any diagnostic list, the value is available on the ungated routes
    /// exactly when no diagnostic is an Error, and `accept_errors` always
    /// returns it with every diagnostic.
    #[test]
    fn value_is_withheld_iff_any_error(
        value in any::<u32>(),
        severities in proptest::collection::vec(severity(), 0..8),
    ) {
        let diags: Vec<Diagnostic> = severities.iter().copied().map(diag).collect();
        let any_error = severities.contains(&Severity::Error);
        let loaded = Loaded::new(value, diags.clone());
        prop_assert_eq!(loaded.has_errors(), any_error);
        let expected = if any_error { None } else { Some(value) };
        prop_assert_eq!(ungated_routes(&loaded), [expected; 4]);
        prop_assert_eq!(loaded.accept_errors(), (value, diags));
    }
}

/// There is no other path to `T`: each case under `tests/ui/fail_closed`
/// fails to compile with its committed diagnostic, and the explicit opt-in
/// compiles.
#[test]
fn value_is_unreachable_without_the_explicit_opt_in() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/fail_closed_pass/*.rs");
    cases.compile_fail("tests/ui/fail_closed/*.rs");
}
