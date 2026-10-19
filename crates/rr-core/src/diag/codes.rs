//! The registry of stable diagnostic codes (US-RR-17-6).
//!
//! Every [`Code`] the workspace emits is defined here, once, by the
//! `registry!` invocation below. Each entry is a public constant (so code
//! uses `codes::UNKNOWN_SCALING`, never a string literal) and a row of
//! [`REGISTRY`] carrying its name, severity, the version it first appeared in
//! and its documentation — the `///` comment on the entry, which is both the
//! rustdoc of the constant and the text of `docs/diagnostics.md`.
//!
//! # Rules
//!
//! * A code is three upper-case letters naming its [`Domain`] followed by
//!   four digits: `DEF0001`.
//! * A code is never reused. Once published it keeps its meaning forever; to
//!   retire one, move it from the registry to [`DEPRECATED`] with the version
//!   and reason (and the replacement, if any). A deprecated code is never
//!   handed out again.
//! * The severity listed is the one the code is always reported with.
//!
//! [`problems`] checks these rules (unique codes and names, well-formed codes,
//! known domains, non-empty docs, no deprecated code back in use); the unit
//! tests run it on the real tables.
//!
//! # Documentation
//!
//! `docs/diagnostics.md` is [`render_markdown`]'s output, committed.
//! Regenerate it with `cargo xtask diagnostics-doc` after changing this file;
//! `cargo xtask diagnostics-doc --check` and the `xtask/tests/diagnostics_doc.rs`
//! test (run by CI's nextest job) fail when the committed file has drifted.

use core::fmt;
use core::fmt::Write as _;

use super::{Code, Severity};

/// A code family: the three-letter prefix of a [`Code`] and what it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    /// The prefix, e.g. `DEF`.
    pub prefix: &'static str,
    /// Heading used in the docs, e.g. `Definitions`.
    pub title: &'static str,
    /// One sentence on what the domain covers.
    pub description: &'static str,
}

/// Every code domain, in documentation order.
pub const DOMAINS: &[Domain] = &[
    Domain {
        prefix: "DEF",
        title: "Definitions",
        description: "Reading ROM, logger and device definitions (rr-defs): \
                      malformed or inconsistent definition content.",
    },
    Domain {
        prefix: "ENC",
        title: "Encoding",
        description: "Storage types, endianness and bit layouts (C3, \
                      `rr-core::encoding`): values a definition asks for that \
                      cannot be encoded or decoded.",
    },
    Domain {
        prefix: "LAY",
        title: "Layering",
        description: "Layered documents and the resolver (C1, \
                      `rr-core::layer`): `extends`/`base` chains, overrides \
                      and omits.",
    },
];

/// One registered, active diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeInfo {
    /// The code.
    pub code: Code,
    /// The constant's name, e.g. `UNKNOWN_SCALING`.
    pub name: &'static str,
    /// The severity the code is always reported with.
    pub severity: Severity,
    /// Workspace version the code first appeared in.
    pub since: &'static str,
    /// The entry's doc comment, one element per line, leading space removed
    /// by [`CodeInfo::doc_lines`].
    pub doc: &'static [&'static str],
}

impl CodeInfo {
    /// The doc comment's lines with the single leading space that `///`
    /// leaves removed.
    pub fn doc_lines(&self) -> impl Iterator<Item = &'static str> {
        self.doc.iter().map(|l| l.strip_prefix(' ').unwrap_or(l))
    }

    /// The first line of the doc comment: a one-sentence summary.
    #[must_use]
    pub fn summary(&self) -> &'static str {
        self.doc_lines().next().unwrap_or("").trim()
    }
}

/// A retired code. It stays listed forever so it is never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeprecatedCode {
    /// The retired code.
    pub code: Code,
    /// The name it had.
    pub name: &'static str,
    /// Workspace version in which it stopped being emitted.
    pub deprecated_in: &'static str,
    /// Why it was retired.
    pub reason: &'static str,
    /// The active code that replaces it, if any.
    pub replaced_by: Option<Code>,
}

/// Defines the code constants and [`REGISTRY`] from one list, so a code
/// cannot exist without its registry row and documentation.
macro_rules! registry {
    (
        $(
            $(#[doc = $doc:literal])+
            $name:ident = $code:literal, $severity:ident, since $since:literal;
        )*
    ) => {
        $(
            $(#[doc = $doc])+
            #[doc = ""]
            #[doc = concat!("Code `", $code, "`, severity `", stringify!($severity), "`.")]
            pub const $name: Code = Code::new($code);
        )*

        /// Every active diagnostic code, with its documentation.
        pub const REGISTRY: &[CodeInfo] = &[
            $(
                CodeInfo {
                    code: $name,
                    name: stringify!($name),
                    severity: Severity::$severity,
                    since: $since,
                    doc: &[$($doc),+],
                },
            )*
        ];
    };
}

registry! {
    /// A table or parameter references a scaling that no layer defines.
    ///
    /// The definition names a scaling by id but the resolved definition has
    /// no scaling with that id. The table is not loaded: decoding it with a
    /// guessed scaling would show wrong values.
    UNKNOWN_SCALING = "DEF0001", Error, since "0.1.0";

    /// A definition names a storage type the encoding does not support.
    ///
    /// The storage string (for example `uint48` or a misspelt `unit16`) does
    /// not name any supported storage type, or names one in a combination
    /// (endianness, bit layout) that is not supported. The table is not
    /// decoded; its values are never shown as zero (MF-02).
    UNSUPPORTED_STORAGE_TYPE = "ENC0001", Error, since "0.1.0";

    /// A definition names a byte order other than big or little endian.
    ///
    /// The endian string (for example `middle`, or legacy's internal codes
    /// `1` / `2`) is neither `big` nor `little`. The table is not decoded:
    /// guessing a byte order (legacy assumed little-endian) shows scrambled
    /// values.
    UNKNOWN_ENDIAN = "ENC0002", Error, since "0.1.0";

    /// A bitfield mask or shift cannot apply to the storage it is on.
    ///
    /// The mask is not a hexadecimal number, is zero, has bits outside the
    /// stored value, or is on a float storage; or the shift would drop field
    /// bits; or a signed field's mask is not one contiguous run of at least
    /// two bits. The table is not decoded: legacy silently clamped such masks
    /// and showed values from the wrong bits.
    INVALID_BITFIELD = "ENC0003", Error, since "0.1.0";

    /// A definition names a table data layout that does not exist.
    ///
    /// The layout string is not `bosch_subtract` (or empty / `default`). The
    /// table is not decoded: legacy fell back to the default layout, which
    /// shows a Bosch-subtract table as meaningless values.
    UNKNOWN_LAYOUT = "ENC0004", Error, since "0.1.0";

    /// A cell does not fit in the bytes it is read from or written to.
    ///
    /// The cell needs more bytes than the image (or buffer) has from the
    /// cell's address, for example a table whose address runs past the end of
    /// the ROM. Nothing is read or written: legacy padded or wrapped such
    /// reads and showed values that are not in the image.
    CELL_OUT_OF_BOUNDS = "ENC0005", Error, since "0.1.0";

    /// A raw value does not fit the storage type or bitfield it is written to.
    ///
    /// The value is below the minimum or above the maximum of the storage
    /// (for example 256 into `u8`, or -1 into `u16`), or has bits outside the
    /// bitfield. Nothing is written: the value is never truncated or wrapped
    /// to fit.
    RAW_OUT_OF_RANGE = "ENC0006", Error, since "0.1.0";

    /// A raw value of the wrong kind for its storage type.
    ///
    /// An integer raw value was written to float storage, a float to integer
    /// storage, or an `f32` to `f64` storage (or the reverse). Nothing is
    /// written: converting between kinds silently would change the value.
    RAW_KIND_MISMATCH = "ENC0007", Error, since "0.1.0";

    /// One cell of a whole-table layout was decoded or encoded on its own.
    ///
    /// In the Bosch-subtract layout each value depends on every later cell of
    /// the table, so a single cell has no value by itself. Nothing is read or
    /// written; the table must be decoded or encoded as a whole.
    WHOLE_TABLE_LAYOUT = "ENC0008", Error, since "0.1.0";

    /// An edit was refused because a value is a NaN, infinite or subnormal float.
    ///
    /// The value stored in the cell, or the value the edit would write, is a
    /// NaN (quiet or signalling), `+Inf`/`-Inf` or a subnormal (denormal)
    /// float, and special floats were not explicitly allowed (PROJECT.md
    /// EC-07). Such values are decoded and displayed exactly as stored;
    /// nothing is written unless the user explicitly allows editing them.
    SPECIAL_FLOAT_EDIT = "ENC0009", Error, since "0.1.0";

    /// An `extends`/`base` chain loops back on itself.
    ///
    /// Following the inheritance chain from a document reaches the same
    /// document again. The document is not resolved; the provenance points
    /// at the reference that closes the loop.
    INHERITANCE_CYCLE = "LAY0001", Error, since "0.1.0";
}

/// Retired codes, which may never be reused. Empty until a code is retired.
pub const DEPRECATED: &[DeprecatedCode] = &[];

/// The registry entry for `code`, if it is active.
#[must_use]
pub fn lookup(code: Code) -> Option<&'static CodeInfo> {
    REGISTRY.iter().find(|info| info.code == code)
}

/// The deprecation entry for `code`, if it has been retired.
#[must_use]
pub fn deprecation(code: Code) -> Option<&'static DeprecatedCode> {
    DEPRECATED.iter().find(|d| d.code == code)
}

/// A broken registry rule, reported by [`problems`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryProblem {
    /// The same code appears twice among the active and deprecated entries.
    DuplicateCode(Code),
    /// Two active entries share a constant name.
    DuplicateName(&'static str),
    /// A code is not three upper-case letters and four digits.
    MalformedCode(Code),
    /// A code's prefix is not a [`DOMAINS`] prefix.
    UnknownDomain(Code),
    /// An active entry has no documentation summary.
    MissingDoc(Code),
    /// A deprecated entry has no reason.
    MissingReason(Code),
    /// A deprecated entry points at a replacement that is not active.
    UnknownReplacement {
        /// The deprecated code.
        code: Code,
        /// Its claimed replacement.
        replaced_by: Code,
    },
}

impl fmt::Display for RegistryProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCode(c) => write!(f, "code {c} is registered more than once"),
            Self::DuplicateName(n) => write!(f, "name {n} is registered more than once"),
            Self::MalformedCode(c) => write!(
                f,
                "code {:?} is not three upper-case letters and four digits",
                c.as_str()
            ),
            Self::UnknownDomain(c) => write!(f, "code {c} has a prefix not listed in DOMAINS"),
            Self::MissingDoc(c) => write!(f, "code {c} has no documentation"),
            Self::MissingReason(c) => write!(f, "deprecated code {c} has no reason"),
            Self::UnknownReplacement { code, replaced_by } => write!(
                f,
                "deprecated code {code} is replaced by {replaced_by}, which is not an active code"
            ),
        }
    }
}

/// `true` when `code` is three ASCII upper-case letters and four digits.
#[must_use]
pub fn is_well_formed(code: Code) -> bool {
    let b = code.as_str().as_bytes();
    b.len() == 7
        && b.iter().take(3).all(u8::is_ascii_uppercase)
        && b.iter().skip(3).all(u8::is_ascii_digit)
}

fn prefix(code: Code) -> &'static str {
    code.as_str().get(..3).unwrap_or("")
}

/// Every rule `registry`, `deprecated` and `domains` break. Empty when the
/// tables are sound. Pure, so tests can feed it broken tables.
#[must_use]
pub fn problems(
    registry: &[CodeInfo],
    deprecated: &[DeprecatedCode],
    domains: &[Domain],
) -> Vec<RegistryProblem> {
    let mut out = Vec::new();
    let all_codes = registry
        .iter()
        .map(|i| i.code)
        .chain(deprecated.iter().map(|d| d.code));
    let mut seen: Vec<Code> = Vec::new();
    for code in all_codes {
        if seen.contains(&code) {
            if !out.contains(&RegistryProblem::DuplicateCode(code)) {
                out.push(RegistryProblem::DuplicateCode(code));
            }
        } else {
            seen.push(code);
        }
        if !is_well_formed(code) {
            out.push(RegistryProblem::MalformedCode(code));
        } else if !domains.iter().any(|d| d.prefix == prefix(code)) {
            out.push(RegistryProblem::UnknownDomain(code));
        }
    }
    let mut names: Vec<&str> = Vec::new();
    for info in registry {
        if names.contains(&info.name) {
            out.push(RegistryProblem::DuplicateName(info.name));
        } else {
            names.push(info.name);
        }
        if info.summary().is_empty() {
            out.push(RegistryProblem::MissingDoc(info.code));
        }
    }
    for d in deprecated {
        if d.reason.trim().is_empty() {
            out.push(RegistryProblem::MissingReason(d.code));
        }
        if let Some(replaced_by) = d.replaced_by
            && !registry.iter().any(|i| i.code == replaced_by)
        {
            out.push(RegistryProblem::UnknownReplacement {
                code: d.code,
                replaced_by,
            });
        }
    }
    out
}

/// `docs/diagnostics.md` for the real tables.
#[must_use]
pub fn render_markdown() -> String {
    render_markdown_from(REGISTRY, DEPRECATED, DOMAINS)
}

/// `docs/diagnostics.md` for the given tables: a summary table and one
/// section per code, grouped by domain and sorted by code, then the
/// deprecated codes. Deterministic, `\n` line endings, one trailing newline.
#[must_use]
pub fn render_markdown_from(
    registry: &[CodeInfo],
    deprecated: &[DeprecatedCode],
    domains: &[Domain],
) -> String {
    let mut s = String::new();
    // Writing to a String cannot fail; `let _ =` keeps clippy quiet without
    // an unwrap.
    let _ = render_into(&mut s, registry, deprecated, domains);
    s
}

fn render_into(
    s: &mut String,
    registry: &[CodeInfo],
    deprecated: &[DeprecatedCode],
    domains: &[Domain],
) -> fmt::Result {
    writeln!(
        s,
        "<!-- Generated by `cargo xtask diagnostics-doc` from \
         crates/rr-core/src/diag/codes.rs. Do not edit by hand. -->"
    )?;
    writeln!(s)?;
    writeln!(s, "# Diagnostic codes")?;
    writeln!(s)?;
    writeln!(
        s,
        "Every diagnostic (concept C15) carries a stable code. Tests and tools \
         match on the code; the message is for people and may be reworded. A \
         code keeps its meaning forever: retired codes are listed under \
         [Deprecated codes](#deprecated-codes) and are never reused."
    )?;
    writeln!(s)?;
    writeln!(
        s,
        "An `error` makes the owning load fail closed: the caller gets no \
         usable value without explicitly accepting the errors."
    )?;

    let mut sorted: Vec<&CodeInfo> = registry.iter().collect();
    sorted.sort_by_key(|i| i.code);

    writeln!(s)?;
    writeln!(s, "| Code | Name | Severity | Summary |")?;
    writeln!(s, "| --- | --- | --- | --- |")?;
    for d in domains {
        for i in sorted.iter().filter(|i| prefix(i.code) == d.prefix) {
            writeln!(
                s,
                "| [`{code}`](#{anchor}) | `{name}` | {sev} | {summary} |",
                code = i.code,
                anchor = i.code.as_str().to_ascii_lowercase(),
                name = i.name,
                sev = i.severity,
                summary = i.summary(),
            )?;
        }
    }

    for d in domains {
        let codes: Vec<&&CodeInfo> = sorted
            .iter()
            .filter(|i| prefix(i.code) == d.prefix)
            .collect();
        writeln!(s)?;
        writeln!(s, "## {}: {}", d.prefix, d.title)?;
        writeln!(s)?;
        writeln!(s, "{}", d.description)?;
        if codes.is_empty() {
            writeln!(s)?;
            writeln!(s, "No codes yet.")?;
        }
        for i in codes {
            writeln!(s)?;
            writeln!(s, "### {}", i.code)?;
            writeln!(s)?;
            writeln!(
                s,
                "`{}` · severity `{}` · since {}",
                i.name, i.severity, i.since
            )?;
            writeln!(s)?;
            let lines: Vec<&str> = i.doc_lines().map(str::trim_end).collect();
            let end = lines
                .iter()
                .rposition(|l| !l.is_empty())
                .map_or(0, |p| p + 1);
            for line in lines.iter().take(end) {
                writeln!(s, "{line}")?;
            }
        }
    }

    writeln!(s)?;
    writeln!(s, "## Deprecated codes")?;
    writeln!(s)?;
    if deprecated.is_empty() {
        writeln!(s, "None.")?;
    } else {
        let mut retired: Vec<&DeprecatedCode> = deprecated.iter().collect();
        retired.sort_by_key(|d| d.code);
        writeln!(s, "| Code | Name | Deprecated in | Replaced by | Reason |")?;
        writeln!(s, "| --- | --- | --- | --- | --- |")?;
        for d in retired {
            let replaced = d
                .replaced_by
                .map_or_else(|| "—".to_owned(), |c| format!("`{c}`"));
            writeln!(
                s,
                "| `{}` | `{}` | {} | {} | {} |",
                d.code,
                d.name,
                d.deprecated_in,
                replaced,
                d.reason.trim()
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(code: &'static str, name: &'static str) -> CodeInfo {
        CodeInfo {
            code: Code::new(code),
            name,
            severity: Severity::Error,
            since: "0.1.0",
            doc: &[" Something went wrong.", "", " More detail."],
        }
    }

    fn retired(code: &'static str, replaced_by: Option<&'static str>) -> DeprecatedCode {
        DeprecatedCode {
            code: Code::new(code),
            name: "OLD",
            deprecated_in: "0.2.0",
            reason: "split into finer codes",
            replaced_by: replaced_by.map(Code::new),
        }
    }

    #[test]
    fn real_registry_has_no_problems() {
        let found = problems(REGISTRY, DEPRECATED, DOMAINS);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn real_registry_is_not_empty_and_every_domain_is_unique() {
        assert!(!REGISTRY.is_empty());
        for (i, d) in DOMAINS.iter().enumerate() {
            assert_eq!(d.prefix.len(), 3);
            assert!(d.prefix.bytes().all(|b| b.is_ascii_uppercase()));
            assert!(
                DOMAINS.iter().skip(i + 1).all(|o| o.prefix != d.prefix),
                "domain {} listed twice",
                d.prefix
            );
        }
    }

    #[test]
    fn constants_are_the_registry_rows() {
        assert_eq!(UNKNOWN_SCALING.as_str(), "DEF0001");
        assert_eq!(UNSUPPORTED_STORAGE_TYPE.as_str(), "ENC0001");
        assert_eq!(UNKNOWN_ENDIAN.as_str(), "ENC0002");
        assert_eq!(INVALID_BITFIELD.as_str(), "ENC0003");
        assert_eq!(UNKNOWN_LAYOUT.as_str(), "ENC0004");
        assert_eq!(CELL_OUT_OF_BOUNDS.as_str(), "ENC0005");
        assert_eq!(RAW_OUT_OF_RANGE.as_str(), "ENC0006");
        assert_eq!(RAW_KIND_MISMATCH.as_str(), "ENC0007");
        assert_eq!(WHOLE_TABLE_LAYOUT.as_str(), "ENC0008");
        assert_eq!(SPECIAL_FLOAT_EDIT.as_str(), "ENC0009");
        assert_eq!(INHERITANCE_CYCLE.as_str(), "LAY0001");
        let row = lookup(UNSUPPORTED_STORAGE_TYPE).unwrap();
        assert_eq!(row.name, "UNSUPPORTED_STORAGE_TYPE");
        assert_eq!(row.severity, Severity::Error);
        assert_eq!(
            row.summary(),
            "A definition names a storage type the encoding does not support."
        );
        assert!(lookup(Code::new("DEF9999")).is_none());
        assert!(deprecation(UNKNOWN_SCALING).is_none());
    }

    #[test]
    fn duplicate_code_is_reported_once() {
        let reg = [
            info("DEF0001", "A"),
            info("DEF0001", "B"),
            info("DEF0001", "C"),
        ];
        assert_eq!(
            problems(&reg, &[], DOMAINS),
            vec![RegistryProblem::DuplicateCode(Code::new("DEF0001"))]
        );
    }

    #[test]
    fn deprecated_code_reused_as_active_is_a_duplicate() {
        let reg = [info("DEF0001", "A")];
        let dep = [retired("DEF0001", None)];
        assert_eq!(
            problems(&reg, &dep, DOMAINS),
            vec![RegistryProblem::DuplicateCode(Code::new("DEF0001"))]
        );
    }

    #[test]
    fn duplicate_name_is_reported() {
        let reg = [info("DEF0001", "A"), info("DEF0002", "A")];
        assert_eq!(
            problems(&reg, &[], DOMAINS),
            vec![RegistryProblem::DuplicateName("A")]
        );
    }

    #[test]
    fn malformed_and_unknown_domain_codes_are_reported() {
        let reg = [
            info("DEF001", "A"),
            info("def0001", "B"),
            info("DEF00012", "C"),
            info("D3F0001", "D"),
            info("XYZ0001", "E"),
            info("DEFA001", "F"),
        ];
        assert_eq!(
            problems(&reg, &[], DOMAINS),
            vec![
                RegistryProblem::MalformedCode(Code::new("DEF001")),
                RegistryProblem::MalformedCode(Code::new("def0001")),
                RegistryProblem::MalformedCode(Code::new("DEF00012")),
                RegistryProblem::MalformedCode(Code::new("D3F0001")),
                RegistryProblem::UnknownDomain(Code::new("XYZ0001")),
                RegistryProblem::MalformedCode(Code::new("DEFA001")),
            ]
        );
    }

    #[test]
    fn missing_doc_reason_and_bad_replacement_are_reported() {
        let mut undocumented = info("DEF0001", "A");
        undocumented.doc = &["   "];
        let mut no_reason = retired("DEF0002", Some("DEF0001"));
        no_reason.reason = " ";
        let dangling = retired("DEF0003", Some("DEF0042"));
        assert_eq!(
            problems(&[undocumented], &[no_reason, dangling], DOMAINS),
            vec![
                RegistryProblem::MissingDoc(Code::new("DEF0001")),
                RegistryProblem::MissingReason(Code::new("DEF0002")),
                RegistryProblem::UnknownReplacement {
                    code: Code::new("DEF0003"),
                    replaced_by: Code::new("DEF0042"),
                },
            ]
        );
    }

    #[test]
    fn valid_deprecation_with_replacement_is_accepted() {
        let reg = [info("DEF0002", "NEW")];
        let dep = [retired("DEF0001", Some("DEF0002"))];
        assert!(problems(&reg, &dep, DOMAINS).is_empty());
    }

    #[test]
    fn problems_display_name_the_code() {
        let all = [
            RegistryProblem::DuplicateCode(Code::new("DEF0001")),
            RegistryProblem::DuplicateName("A"),
            RegistryProblem::MalformedCode(Code::new("DEF1")),
            RegistryProblem::UnknownDomain(Code::new("XYZ0001")),
            RegistryProblem::MissingDoc(Code::new("DEF0001")),
            RegistryProblem::MissingReason(Code::new("DEF0001")),
            RegistryProblem::UnknownReplacement {
                code: Code::new("DEF0001"),
                replaced_by: Code::new("DEF0002"),
            },
        ];
        let rendered: Vec<String> = all.iter().map(ToString::to_string).collect();
        assert_eq!(
            rendered,
            [
                "code DEF0001 is registered more than once",
                "name A is registered more than once",
                "code \"DEF1\" is not three upper-case letters and four digits",
                "code XYZ0001 has a prefix not listed in DOMAINS",
                "code DEF0001 has no documentation",
                "deprecated code DEF0001 has no reason",
                "deprecated code DEF0001 is replaced by DEF0002, which is not an active code",
            ]
        );
    }

    #[test]
    fn doc_lines_strip_one_leading_space() {
        let i = info("DEF0001", "A");
        let lines: Vec<&str> = i.doc_lines().collect();
        assert_eq!(lines, ["Something went wrong.", "", "More detail."]);
        assert_eq!(i.summary(), "Something went wrong.");
        let mut empty = i;
        empty.doc = &[];
        assert_eq!(empty.summary(), "");
    }

    #[test]
    fn markdown_lists_every_code_and_deprecation() {
        let reg = [info("DEF0002", "NEW"), info("DEF0001", "FIRST")];
        let dep = [
            retired("DEF0003", Some("DEF0002")),
            retired("DEF0000", None),
        ];
        let md = render_markdown_from(&reg, &dep, &DOMAINS[..2]);
        let expected = "\
<!-- Generated by `cargo xtask diagnostics-doc` from crates/rr-core/src/diag/codes.rs. Do not edit by hand. -->

# Diagnostic codes

Every diagnostic (concept C15) carries a stable code. Tests and tools match on the code; the message is for people and may be reworded. A code keeps its meaning forever: retired codes are listed under [Deprecated codes](#deprecated-codes) and are never reused.

An `error` makes the owning load fail closed: the caller gets no usable value without explicitly accepting the errors.

| Code | Name | Severity | Summary |
| --- | --- | --- | --- |
| [`DEF0001`](#def0001) | `FIRST` | error | Something went wrong. |
| [`DEF0002`](#def0002) | `NEW` | error | Something went wrong. |

## DEF: Definitions

Reading ROM, logger and device definitions (rr-defs): malformed or inconsistent definition content.

### DEF0001

`FIRST` · severity `error` · since 0.1.0

Something went wrong.

More detail.

### DEF0002

`NEW` · severity `error` · since 0.1.0

Something went wrong.

More detail.

## ENC: Encoding

Storage types, endianness and bit layouts (C3, `rr-core::encoding`): values a definition asks for that cannot be encoded or decoded.

No codes yet.

## Deprecated codes

| Code | Name | Deprecated in | Replaced by | Reason |
| --- | --- | --- | --- | --- |
| `DEF0000` | `OLD` | 0.2.0 | — | split into finer codes |
| `DEF0003` | `OLD` | 0.2.0 | `DEF0002` | split into finer codes |
";
        assert_eq!(md, expected);
    }

    #[test]
    fn real_markdown_documents_every_registered_code() {
        let md = render_markdown();
        assert!(md.ends_with("None.\n"));
        for i in REGISTRY {
            assert!(md.contains(&format!("### {}\n", i.code)), "{}", i.code);
            assert!(md.contains(i.summary()), "{}", i.code);
        }
        for d in DOMAINS {
            assert!(md.contains(&format!("## {}: {}\n", d.prefix, d.title)));
        }
    }
}
