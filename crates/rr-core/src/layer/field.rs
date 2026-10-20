//! Single-valued field states: [`Field`] (replace) and [`Nested`]
//! (deep-merge).

use super::Merge;

/// A field merged by **replace**: the upper layer's value wins.
///
/// | overlay | result |
/// |---------|--------|
/// | `Unset` | the inherited state |
/// | `Set(v)` | `Set(v)` |
/// | `Omit` | `Omit` |
///
/// An override is unconditional: it does not depend on the inherited value
/// or on any other field (MF-06; legacy inherited signedness only while the
/// base's storage type was unset, `xml/TableScaleUnmarshaller.java:153`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Field<T> {
    /// Not mentioned: inherit.
    #[default]
    Unset,
    /// Set to a value, replacing any inherited one.
    Set(T),
    /// Explicitly removed: the field has no value, whatever was inherited.
    Omit,
}

impl<T> Field<T> {
    /// The value, if set.
    #[must_use]
    pub const fn get(&self) -> Option<&T> {
        match self {
            Self::Set(v) => Some(v),
            Self::Unset | Self::Omit => None,
        }
    }

    /// `true` if not mentioned.
    #[must_use]
    pub const fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }

    /// `true` if explicitly removed.
    #[must_use]
    pub const fn is_omitted(&self) -> bool {
        matches!(self, Self::Omit)
    }
}

impl<T: Clone> Merge for Field<T> {
    fn merge_from(&mut self, overlay: &Self) {
        if !overlay.is_unset() {
            self.clone_from(overlay);
        }
    }
}

/// A nested document merged by **deep-merge**: the upper layer patches the
/// inherited document field by field.
///
/// | inherited \ overlay | `Unset` | `Patch(v)` | `Replace(v)` | `Omit` |
/// |---------------------|---------|------------|--------------|--------|
/// | `Unset` | `Unset` | `Patch(v)` | `Replace(v)` | `Omit` |
/// | `Patch(u)` | `Patch(u)` | `Patch(u ⊕ v)` | `Replace(v)` | `Omit` |
/// | `Replace(u)` | `Replace(u)` | `Replace(u ⊕ v)` | `Replace(v)` | `Omit` |
/// | `Omit` | `Omit` | `Replace(v)` | `Replace(v)` | `Omit` |
///
/// [`Replace`](Self::Replace) is "drop what was inherited, then patch": a
/// patch over an omitted document starts from nothing, and keeps doing so
/// under any further base. That state is what makes the merge associative
/// (module docs, *Laws*): `(Patch(u) ⊕ Omit) ⊕ Patch(v)` and
/// `Patch(u) ⊕ (Omit ⊕ Patch(v))` are both `Replace(v)`.
///
/// Once resolution is finished, `Patch` and `Replace` both mean "present"
/// ([`get`](Self::get)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Nested<P> {
    /// Not mentioned: inherit.
    #[default]
    Unset,
    /// Merged on top of the inherited document (or on the empty document,
    /// if nothing is inherited).
    Patch(P),
    /// Replaces the inherited document, which is dropped.
    Replace(P),
    /// Explicitly removed, whatever was inherited.
    Omit,
}

impl<P> Nested<P> {
    /// The document, if present (patched or replaced).
    #[must_use]
    pub const fn get(&self) -> Option<&P> {
        match self {
            Self::Patch(p) | Self::Replace(p) => Some(p),
            Self::Unset | Self::Omit => None,
        }
    }

    /// `true` if not mentioned.
    #[must_use]
    pub const fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }

    /// `true` if explicitly removed.
    #[must_use]
    pub const fn is_omitted(&self) -> bool {
        matches!(self, Self::Omit)
    }
}

impl<P: Merge + Clone> Merge for Nested<P> {
    fn merge_from(&mut self, overlay: &Self) {
        match overlay {
            Self::Unset => {}
            Self::Replace(_) | Self::Omit => self.clone_from(overlay),
            Self::Patch(v) => match self {
                Self::Unset => *self = Self::Patch(v.clone()),
                Self::Omit => *self = Self::Replace(v.clone()),
                Self::Patch(u) | Self::Replace(u) => u.merge_from(v),
            },
        }
    }
}
