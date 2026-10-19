//! Golden-snapshot helpers built on [`insta`] (PROJECT.md "Testing Strategy", tier T2).
//!
//! Every snapshot test in the workspace starts from [`settings`], so snapshot
//! files look the same in every crate and never contain values that change
//! between machines or runs.
//!
//! # Conventions
//!
//! * **Location.** Snapshots live in insta's default `snapshots/` directory
//!   next to the test source file and are committed. A test run must leave no
//!   `*.snap.new` / `*.pending-snap` files behind: review with
//!   `cargo insta review` (or `cargo insta test -p <crate> --accept` after
//!   reading the diff) before committing.
//! * **Naming.** The module path is prepended to the snapshot name
//!   (`<crate>__<module>__<name>.snap`) so two tests named alike in
//!   different modules can never collide.
//! * **Determinism.** Maps are serialized with sorted keys, so a `HashMap`
//!   never reorders a snapshot. The asserted expression is kept in the
//!   header to make reviews readable.
//! * **Redaction placeholders.** Values that vary by machine or run are
//!   replaced by a fixed placeholder in square brackets:
//!
//!   | Value                                   | Placeholder        | Helper                |
//!   |-----------------------------------------|--------------------|-----------------------|
//!   | Absolute path under a temp/checkout dir | [`PATH`] `[path]`  | [`redact_paths`]      |
//!   | Civil timestamp (`WallTime`, RFC 3339)  | [`WALL_TIME`] `[wall-time]` | [`redact_wall_times`] |
//!   | Measured duration (`Duration` `Debug`)  | [`DURATION`] `[duration]`   | [`redact_durations`]  |
//!
//!   Text helpers install insta *filters* (regex replacements over the
//!   rendered snapshot), so they work for `assert_snapshot!`,
//!   `assert_debug_snapshot!` and the serde formats alike. For structured
//!   (`assert_yaml_snapshot!`) snapshots prefer a selector redaction, which
//!   replaces a field regardless of its content: [`redact_fields`].
//! * **Virtual time is not redacted.** `MonoInstant`s and durations produced
//!   by a VirtualClock are deterministic and are part of what a replay or
//!   simulator test asserts; only *real* (measured) time is redacted.
//!
//! # Example
//!
//! ```
//! let mut settings = rr_testkit::snapshot::settings();
//! rr_testkit::snapshot::redact_durations(&mut settings);
//! settings.bind(|| {
//!     insta::assert_snapshot!("took [duration]", @"took [duration]");
//! });
//! ```

use std::path::Path;

use insta::Settings;

/// Placeholder for an absolute filesystem path prefix.
pub const PATH: &str = "[path]";
/// Placeholder for a civil (wall-clock) timestamp.
pub const WALL_TIME: &str = "[wall-time]";
/// Placeholder for a measured (real-time) duration.
pub const DURATION: &str = "[duration]";

/// RFC 3339 / ISO 8601 date-time, with optional fraction and offset.
const WALL_TIME_REGEX: &str =
    r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?";
/// `std::time::Duration`'s `Debug` form (`1.5s`, `250ms`, `12.3µs`, `7ns`).
const DURATION_REGEX: &str = r"\b\d+(?:\.\d+)?(?:ns|µs|us|ms|s)\b";

/// The workspace-standard snapshot settings (see the module docs).
///
/// Starts from the currently bound settings, so it composes with an outer
/// `Settings::bind`. Bind the result around the assertions:
/// `settings().bind(|| insta::assert_debug_snapshot!(value))`.
#[must_use]
pub fn settings() -> Settings {
    let mut settings = Settings::clone_current();
    settings.set_sort_maps(true);
    settings.set_prepend_module_to_snapshot(true);
    settings.set_omit_expression(false);
    settings
}

/// Replaces every occurrence of `root` (e.g. a `tempfile` directory or
/// `CARGO_MANIFEST_DIR`) with [`PATH`], keeping the relative tail.
///
/// Both the native spelling and, where they differ, the `/`-separated and
/// `Debug`-escaped (`\\`) spellings are matched, so Windows paths printed
/// with `{:?}` are redacted too. An empty `root` installs nothing.
pub fn redact_paths(settings: &mut Settings, root: &Path) {
    let native = root.to_string_lossy();
    if native.is_empty() {
        return;
    }
    let mut spellings = vec![native.to_string()];
    for variant in [native.replace('\\', "/"), native.replace('\\', r"\\")] {
        if !spellings.contains(&variant) {
            spellings.push(variant);
        }
    }
    // Longest first, so `C:\\x` is not half-replaced by its `C:\x` spelling.
    spellings.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for spelling in spellings {
        settings.add_filter(&regex_escape(&spelling), PATH);
    }
}

/// Replaces RFC 3339 / ISO 8601 timestamps (e.g. `2026-10-19T08:48:44.5Z`)
/// with [`WALL_TIME`].
pub fn redact_wall_times(settings: &mut Settings) {
    settings.add_filter(WALL_TIME_REGEX, WALL_TIME);
}

/// Replaces `Duration`-style values (`1.5s`, `250ms`, `12.3µs`, `7ns`) with
/// [`DURATION`].
///
/// Use only for *measured* durations: virtual-clock durations are
/// deterministic and belong in the snapshot.
pub fn redact_durations(settings: &mut Settings) {
    settings.add_filter(DURATION_REGEX, DURATION);
}

/// Replaces the value at each insta `selector` (e.g. `".started_at"`,
/// `".frames[].t"`) with `placeholder` in structured (serde) snapshots.
///
/// Pass one of [`PATH`], [`WALL_TIME`] or [`DURATION`] unless the value is
/// of a kind those don't cover.
pub fn redact_fields(settings: &mut Settings, selectors: &[&str], placeholder: &'static str) {
    for selector in selectors {
        settings.add_redaction(selector, placeholder);
    }
}

/// Escapes regex metacharacters so `literal` matches itself.
fn regex_escape(literal: &str) -> String {
    let mut escaped = String::with_capacity(literal.len() * 2);
    for c in literal.chars() {
        if matches!(
            c,
            '\\' | '.'
                | '+'
                | '*'
                | '?'
                | '('
                | ')'
                | '|'
                | '['
                | ']'
                | '{'
                | '}'
                | '^'
                | '$'
                | '#'
                | '&'
                | '-'
                | '~'
        ) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::regex_escape;

    #[test]
    fn regex_escape_escapes_metacharacters() {
        assert_eq!(regex_escape(r"C:\a.b"), r"C:\\a\.b");
        assert_eq!(regex_escape("/tmp/x+y"), r"/tmp/x\+y");
    }
}
