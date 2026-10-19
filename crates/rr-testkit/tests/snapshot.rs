//! Snapshot helper tests: the standard settings and each redaction convention.

use std::collections::HashMap;
use std::path::Path;

use rr_testkit::snapshot::{
    self, DURATION, PATH, WALL_TIME, redact_durations, redact_fields, redact_paths,
    redact_wall_times,
};

/// Unsorted input map; the standard settings must serialize it sorted.
#[test]
fn settings_sort_maps_in_yaml_snapshots() {
    let mut map = HashMap::new();
    map.insert("zeta", 3);
    map.insert("alpha", 1);
    map.insert("mu", 2);
    snapshot::settings().bind(|| insta::assert_yaml_snapshot!("sorted_map", map));
}

#[test]
fn text_redactions_replace_paths_wall_times_and_durations() {
    let root = Path::new("/tmp/rr-test.ab12+cd");
    let report = format!(
        "opened {}/roms/stock.bin at 2026-10-19T08:48:44.562619Z in 12.5ms (budget 2s)\n\
         unrelated: /tmp/rr-test-other/x.bin, 2026 laps, 7 cells",
        root.display()
    );
    let mut settings = snapshot::settings();
    redact_paths(&mut settings, root);
    redact_wall_times(&mut settings);
    redact_durations(&mut settings);
    settings.bind(|| insta::assert_snapshot!("text_redactions", report));
}

#[test]
fn windows_paths_are_redacted_in_debug_output() {
    let root = Path::new(r"C:\Users\ci\AppData\Local\Temp\.tmpX1");
    let shown = format!(
        "{:?} {}",
        r"C:\Users\ci\AppData\Local\Temp\.tmpX1\a.bin",
        "C:/Users/ci/AppData/Local/Temp/.tmpX1/b.bin"
    );
    let mut settings = snapshot::settings();
    redact_paths(&mut settings, root);
    settings.bind(|| {
        insta::assert_snapshot!(shown, @r#""[path]\\a.bin" [path]/b.bin"#);
    });
}

#[test]
fn field_redactions_replace_structured_values() {
    let mut record = HashMap::new();
    record.insert("rom", "/home/someone/roms/stock.bin");
    record.insert("started_at", "2026-10-19T08:48:44Z");
    record.insert("elapsed", "12.5ms");
    record.insert("ecu", "4B12785207");
    let mut settings = snapshot::settings();
    redact_fields(&mut settings, &[".rom"], PATH);
    redact_fields(&mut settings, &[".started_at"], WALL_TIME);
    redact_fields(&mut settings, &[".elapsed"], DURATION);
    settings.bind(|| insta::assert_yaml_snapshot!("field_redactions", record));
}
