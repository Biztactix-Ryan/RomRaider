//! `docs/diagnostics.md` is generated from the diagnostic code registry and
//! committed; this fails CI (nextest `test` job) when it drifts (US-RR-17-6).

use std::path::Path;

use rr_core::diag::codes;
use xtask::diagnostics_doc::{self, DOC_PATH};

/// The registry is sound and the committed page is exactly what it renders.
/// Fix a failure with `cargo xtask diagnostics-doc`.
#[test]
fn committed_diagnostics_doc_matches_the_registry() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let problems = diagnostics_doc::check(&root).unwrap();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The committed page documents every registered code, so the drift check
/// above cannot pass on an empty or truncated registry render.
#[test]
fn committed_doc_lists_every_registered_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let doc = std::fs::read_to_string(root.join(DOC_PATH)).unwrap();
    assert!(!codes::REGISTRY.is_empty());
    for info in codes::REGISTRY {
        assert!(doc.contains(&format!("### {}", info.code)), "{}", info.code);
        assert!(doc.contains(info.summary()), "{}", info.code);
    }
}

/// A hand edit, a missing file and a stale render are each reported; CRLF
/// line endings alone are not drift.
#[test]
fn drift_is_detected() {
    let expected = codes::render_markdown();
    assert!(diagnostics_doc::drift(Some(&expected), &expected).is_empty());
    let crlf = expected.replace('\n', "\r\n");
    assert!(diagnostics_doc::drift(Some(&crlf), &expected).is_empty());

    let edited = expected.replacen("severity `error`", "severity `warning`", 1);
    let p = diagnostics_doc::drift(Some(&edited), &expected);
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("out of date"), "{}", p[0]);
    assert!(p[0].contains("cargo xtask diagnostics-doc"), "{}", p[0]);

    let truncated = expected.lines().take(5).collect::<Vec<_>>().join("\n");
    let p = diagnostics_doc::drift(Some(&truncated), &expected);
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("line 6"), "{}", p[0]);

    let longer = format!("{expected}extra\n");
    assert_eq!(diagnostics_doc::drift(Some(&longer), &expected).len(), 1);

    let p = diagnostics_doc::drift(None, &expected);
    assert_eq!(p.len(), 1);
    assert!(p[0].contains("missing"), "{}", p[0]);
}
