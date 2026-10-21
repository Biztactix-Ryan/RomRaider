//! Reader limits (US-RR-27-5, SECURITY.md *API Security*, EC-09).
//!
//! Definition packs are untrusted input: a file downloaded from a forum can
//! hold anything. [`Limits`] bound what one file can cost to read, so that no
//! file can exhaust the memory, stack or time of the program loading it.
//! Each limit is checked before the work it bounds is done, and the first one
//! exceeded stops the read with its own `XML` diagnostic code:
//!
//! * the input length, before a byte of the input is looked at
//!   (`XML0006`);
//! * the nesting depth, before an element is opened (`XML0007`);
//! * the element count, before an element is built (`XML0008`);
//! * the attributes of one element, and of the whole file, before each
//!   further attribute is read (`XML0009`);
//! * the length of a name or a raw attribute value, before it is copied or
//!   unescaped (`XML0010`);
//! * the total text kept, before each piece of text or attribute value is
//!   added (`XML0011`).
//!
//! The reader never expands an entity other than the five predefined ones
//! and character references, each of which is no longer than the text it
//! replaces, so no limit on entity expansion is needed: nothing a file says
//! can make the tree larger than the file plus the per-element overhead the
//! element and attribute limits bound.
//!
//! # Defaults
//!
//! [`Limits::DEFAULT`], which [`parse`](super::parse) uses, leaves wide
//! headroom over the pinned definition corpus (D-028: the 265 RomRaider
//! ECU, logger and dyno XML files of `[defs]`):
//!
//! | Limit | Default | Largest in the corpus |
//! |-------|---------|-----------------------|
//! | [`max_input_len`](Limits::max_input_len) | 64 MiB | 8.3 MiB (`ecu/standard/ecu_defs.xml`) |
//! | [`max_depth`](Limits::max_depth) | 64 levels | 7 |
//! | [`max_elements`](Limits::max_elements) | 1 000 000 | 103 272 |
//! | [`max_attributes`](Limits::max_attributes) | 64 | 10 |
//! | [`max_total_attributes`](Limits::max_total_attributes) | 2 000 000 | 211 065 |
//! | [`max_value_len`](Limits::max_value_len) | 64 KiB | 824 bytes |
//! | [`max_text_len`](Limits::max_text_len) | 32 MiB | 3.5 MiB |
//!
//! Every corpus file reads with no diagnostic at all under these limits.
//! Together they bound one read to the input plus at most a million
//! elements (about 150 bytes each), two million attributes (about 130 bytes
//! each with their name and value allocations) and 32 MiB of kept text:
//! roughly half a GiB at the very worst however hostile the file, and about
//! 50 MiB for the largest real one. A caller reading less trusted input, or
//! on a smaller machine, can tighten any of them. The reader
//! does not recurse, so depth costs heap, not stack; but the tree's own
//! `Drop` does, so [`max_depth`](Limits::max_depth) is capped at
//! [`MAX_DEPTH`] whatever a caller asks for.

/// The deepest nesting any [`Limits`] can allow. Dropping (or comparing, or
/// debug-printing) an [`Element`](super::Element) recurses once per level,
/// so this ceiling keeps that recursion far from the end of the stack.
pub const MAX_DEPTH: u32 = 1024;

/// Bounds on what one XML file may cost to read.
///
/// ```
/// use rr_core::diag::codes;
/// use rr_defs::xml::{self, Limits};
///
/// let mut limits = Limits::DEFAULT;
/// limits.max_depth = 2;
/// let loaded = xml::parse_with("deep.xml", b"<a><b><c/></b></a>", &limits);
/// assert_eq!(loaded.diagnostics()[0].code(), codes::XML_TOO_DEEP);
/// assert_eq!(loaded.diagnostics()[0].provenance().to_string(), "deep.xml:1:7");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Limits {
    /// The longest input, in bytes as given (before any decoding).
    pub max_input_len: usize,
    /// The deepest element nesting; the root element is depth 1. Capped at
    /// [`MAX_DEPTH`].
    pub max_depth: u32,
    /// The most elements in the whole file.
    pub max_elements: u32,
    /// The most attributes on one element.
    pub max_attributes: u32,
    /// The most attributes in the whole file.
    pub max_total_attributes: u32,
    /// The longest element name, attribute name or raw (still escaped)
    /// attribute value, in bytes.
    pub max_value_len: usize,
    /// The most text kept in the tree, in bytes: element text plus attribute
    /// values, after unescaping. Whitespace-only text between elements is not
    /// kept and does not count.
    pub max_text_len: usize,
}

impl Limits {
    /// The limits [`parse`](super::parse) uses.
    pub const DEFAULT: Self = Self {
        max_input_len: 64 << 20,
        max_depth: 64,
        max_elements: 1_000_000,
        max_attributes: 64,
        max_total_attributes: 2_000_000,
        max_value_len: 64 << 10,
        max_text_len: 32 << 20,
    };

    /// [`max_depth`](Self::max_depth), capped at [`MAX_DEPTH`]: the depth
    /// the reader enforces.
    #[must_use]
    pub fn effective_depth(self) -> u32 {
        self.max_depth.min(MAX_DEPTH)
    }
}

/// [`Limits::DEFAULT`].
impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}
