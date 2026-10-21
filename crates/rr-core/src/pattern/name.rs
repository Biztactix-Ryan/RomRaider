//! Name patterns: an op name, an ID prefix and a set of header names.

use super::{Mismatch, PatternError, PatternKind};

/// How names and text are compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Case {
    /// Byte for byte.
    #[default]
    Sensitive,
    /// ASCII letters compare equal to their other case; every other byte
    /// byte for byte (legacy's `equalsIgnoreCase` on ROM ID strings,
    /// `maps/RomID.java:85`, for the ASCII IDs definitions use).
    AsciiInsensitive,
}

impl Case {
    fn eq(self, a: &[u8], b: &[u8]) -> bool {
        match self {
            Self::Sensitive => a == b,
            Self::AsciiInsensitive => a.eq_ignore_ascii_case(b),
        }
    }

    fn eq_byte(self, a: u8, b: u8) -> bool {
        match self {
            Self::Sensitive => a == b,
            Self::AsciiInsensitive => a.eq_ignore_ascii_case(&b),
        }
    }
}

/// A named operation, matched exactly: the op of a quirk `when` clause
/// (`read_memory`, `init`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpPattern {
    name: Box<str>,
}

impl OpPattern {
    /// Compiles an op pattern.
    ///
    /// # Errors
    ///
    /// [`PatternError::EmptyName`] for an empty name and
    /// [`PatternError::InvalidOpName`] for one containing whitespace or a
    /// control character (an op name is an identifier, so either is a typo).
    pub fn new(name: &str) -> Result<Self, PatternError> {
        if name.is_empty() {
            return Err(PatternError::EmptyName {
                kind: PatternKind::Op,
            });
        }
        if let Some((position, _)) = name
            .char_indices()
            .find(|&(_, c)| c.is_whitespace() || c.is_control())
        {
            return Err(PatternError::InvalidOpName { position });
        }
        Ok(Self { name: name.into() })
    }

    /// The op name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether `op` is this op.
    #[must_use]
    pub fn matches(&self, op: &str) -> bool {
        *self.name == *op
    }

    /// Matches `op`.
    ///
    /// # Errors
    ///
    /// [`Mismatch::OpDiffers`] when `op` is another op.
    pub fn check(&self, op: &str) -> Result<(), Mismatch> {
        if self.matches(op) {
            Ok(())
        } else {
            Err(Mismatch::OpDiffers)
        }
    }
}

/// The start of an ECU or ROM ID string: `4B12` matches `4B12785207`.
///
/// Every ID that starts with the prefix matches, the prefix itself
/// included. Profile selection by ECU ID (D-004) uses it so one profile can
/// cover a family of IDs; an exact ID is a prefix as long as the ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdPrefix {
    prefix: Box<str>,
    case: Case,
}

impl IdPrefix {
    /// Compiles an ID prefix, compared as `case` says.
    ///
    /// # Errors
    ///
    /// [`PatternError::EmptyName`] for an empty prefix, which would match
    /// every ID.
    pub fn new(prefix: &str, case: Case) -> Result<Self, PatternError> {
        if prefix.is_empty() {
            return Err(PatternError::EmptyName {
                kind: PatternKind::IdPrefix,
            });
        }
        Ok(Self {
            prefix: prefix.into(),
            case,
        })
    }

    /// The prefix.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// How the prefix is compared.
    #[must_use]
    pub const fn case(&self) -> Case {
        self.case
    }

    /// Whether `id` starts with the prefix.
    #[must_use]
    pub fn matches(&self, id: &str) -> bool {
        let prefix = self.prefix.as_bytes();
        id.as_bytes()
            .get(..prefix.len())
            .is_some_and(|head| self.case.eq(head, prefix))
    }

    /// Matches `id`.
    ///
    /// # Errors
    ///
    /// [`Mismatch::IdDiffers`] at the first byte that differs from the
    /// prefix, or [`Mismatch::IdTooShort`] when `id` is a strict start of
    /// the prefix (no byte differs, but it ends first).
    pub fn check(&self, id: &str) -> Result<(), Mismatch> {
        let prefix = self.prefix.as_bytes();
        let id = id.as_bytes();
        if let Some(position) = id
            .iter()
            .zip(prefix)
            .position(|(&a, &b)| !self.case.eq_byte(a, b))
        {
            return Err(Mismatch::IdDiffers { position });
        }
        if id.len() < prefix.len() {
            return Err(Mismatch::IdTooShort {
                prefix_len: prefix.len(),
                id_len: id.len(),
            });
        }
        Ok(())
    }
}

/// Header names that must all be in a CSV header row: how a log format is
/// recognised (`Time/s` for AEM, `Seconds` for a Cobb AccessPort log).
///
/// Each name is trimmed of surrounding whitespace when compiled, and each
/// header is trimmed when matched; a required name matches a header that is
/// equal to it as the set's [`Case`] says. Header order and extra headers do
/// not matter. Legacy instead accepted any header *containing* the name
/// (`logger/ecu/ui/tab/dyno/DynoControlPanel.java:1025-1031`), so a column
/// named `Engine time` passed for OP2's `time`; here a name is a whole
/// header. Unquoting and byte-order marks are the CSV reader's business.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeaderSet {
    names: Box<[Box<str>]>,
    case: Case,
}

impl HeaderSet {
    /// Compiles a header set from its required names.
    ///
    /// # Errors
    ///
    /// * [`PatternError::EmptyHeaderSet`] for no names;
    /// * [`PatternError::EmptyName`] for a name that is empty once trimmed;
    /// * [`PatternError::DuplicateHeader`] for a name given twice (compared
    ///   as `case` says).
    pub fn new<S: AsRef<str>>(names: &[S], case: Case) -> Result<Self, PatternError> {
        if names.is_empty() {
            return Err(PatternError::EmptyHeaderSet);
        }
        let mut compiled: Vec<Box<str>> = Vec::with_capacity(names.len());
        for (second, name) in names.iter().enumerate() {
            let name = name.as_ref().trim();
            if name.is_empty() {
                return Err(PatternError::EmptyName {
                    kind: PatternKind::HeaderSet,
                });
            }
            if let Some(first) = compiled
                .iter()
                .position(|seen| case.eq(seen.as_bytes(), name.as_bytes()))
            {
                return Err(PatternError::DuplicateHeader { first, second });
            }
            compiled.push(name.into());
        }
        Ok(Self {
            names: compiled.into_boxed_slice(),
            case,
        })
    }

    /// The required names, trimmed, in the order given.
    #[must_use]
    pub fn names(&self) -> &[Box<str>] {
        &self.names
    }

    /// How names are compared.
    #[must_use]
    pub const fn case(&self) -> Case {
        self.case
    }

    /// Whether every required name is among `headers`.
    #[must_use]
    pub fn matches<S: AsRef<str>>(&self, headers: &[S]) -> bool {
        self.first_missing(headers).is_none()
    }

    /// Matches `headers`.
    ///
    /// # Errors
    ///
    /// [`Mismatch::HeaderMissing`] for the first required name (in
    /// [`names`](Self::names) order) that no header equals.
    pub fn check<S: AsRef<str>>(&self, headers: &[S]) -> Result<(), Mismatch> {
        match self.first_missing(headers) {
            None => Ok(()),
            Some(index) => Err(Mismatch::HeaderMissing { index }),
        }
    }

    fn first_missing<S: AsRef<str>>(&self, headers: &[S]) -> Option<usize> {
        self.names.iter().position(|name| {
            !headers
                .iter()
                .any(|h| self.case.eq(h.as_ref().trim().as_bytes(), name.as_bytes()))
        })
    }
}
