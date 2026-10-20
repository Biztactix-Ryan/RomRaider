//! List field states: [`Keyed`] (items with identity) and [`Appended`]
//! (items without).

use std::collections::BTreeMap;

use super::{Merge, Nested};

/// A list of items with identity, merged **per key**: a ROM definition's
/// tables by name, a profile's quirks by id.
///
/// Each key holds a [`Nested`] state, so an upper layer can patch an
/// inherited item, replace it, or omit it. Merging `overlay` on top:
///
/// * an item whose key is already in the list is merged into it **in
///   place**, so it keeps its position;
/// * an item with a new key is **appended**, in the overlay's order;
/// * an omitted item stays in the list as an omitted entry, so the merge
///   stays associative and a later layer can bring it back in its old
///   position. [`present`](Self::present) skips it.
///
/// Omitting one item removes exactly that item: every item before and after
/// it is kept (MF-07; legacy's `filterFoundRomTables` returned at the first
/// omitted table, so every later table lost its address,
/// `xml/TableScaleUnmarshaller.java:330-333`).
///
/// Keys are unique: [`insert`](Self::insert) on a key already in the list
/// merges into its entry. Lookups during a merge go through an ordered map,
/// so nothing depends on hash order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keyed<K, P> {
    entries: Vec<(K, Nested<P>)>,
}

impl<K, P> Keyed<K, P> {
    /// An empty list.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Every entry in order, including omitted and unset ones.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = (&K, &Nested<P>)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// The items that are present (patched or replaced), in order.
    pub fn present(&self) -> impl Iterator<Item = (&K, &P)> {
        self.entries
            .iter()
            .filter_map(|(k, v)| v.get().map(|p| (k, p)))
    }

    /// The number of entries, including omitted and unset ones.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<K: PartialEq, P> Keyed<K, P> {
    /// The entry for `key`, if the list has one.
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&Nested<P>> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

impl<K: Ord + Clone, P: Merge + Clone> Keyed<K, P> {
    /// Adds `state` for `key`: merged into the entry if `key` is already in
    /// the list, appended otherwise.
    pub fn insert(&mut self, key: K, state: Nested<P>) {
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.merge_from(&state),
            None => self.entries.push((key, state)),
        }
    }

    /// [`insert`](Self::insert)s a patch of `key`.
    pub fn patch(&mut self, key: K, item: P) {
        self.insert(key, Nested::Patch(item));
    }

    /// [`insert`](Self::insert)s an omit of `key`.
    pub fn omit(&mut self, key: K) {
        self.insert(key, Nested::Omit);
    }
}

impl<K, P> Default for Keyed<K, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone, P: Merge + Clone> FromIterator<(K, Nested<P>)> for Keyed<K, P> {
    /// [`insert`](Self::insert)s each entry in turn.
    fn from_iter<I: IntoIterator<Item = (K, Nested<P>)>>(iter: I) -> Self {
        let mut out = Self::new();
        for (k, v) in iter {
            out.insert(k, v);
        }
        out
    }
}

impl<K: Ord + Clone, P: Merge + Clone> Merge for Keyed<K, P> {
    fn merge_from(&mut self, overlay: &Self) {
        let at: Vec<Option<usize>> = {
            let index: BTreeMap<&K, usize> = self
                .entries
                .iter()
                .enumerate()
                .map(|(i, (k, _))| (k, i))
                .collect();
            overlay
                .entries
                .iter()
                .map(|(k, _)| index.get(k).copied())
                .collect()
        };
        for ((key, state), at) in overlay.entries.iter().zip(at) {
            match at.and_then(|i| self.entries.get_mut(i)) {
                Some((_, v)) => v.merge_from(state),
                None => self.entries.push((key.clone(), state.clone())),
            }
        }
    }
}

/// A list of items without identity, merged by **append**: the upper
/// layer's items follow the inherited ones.
///
/// Without a key there is nothing to name one item by, so an upper layer
/// can only add items or drop the inherited list as a whole
/// ([`replace`](Self::replace), [`omit`](Self::omit)). A list whose items
/// need to be removed or patched one at a time is a [`Keyed`] list.
///
/// Appending is not idempotent: a list merged on top of itself has its
/// items twice (module docs, *Laws*).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Appended<T> {
    replace: bool,
    items: Vec<T>,
}

impl<T> Appended<T> {
    /// Appends `items` to the inherited list.
    #[must_use]
    pub const fn append(items: Vec<T>) -> Self {
        Self {
            replace: false,
            items,
        }
    }

    /// Drops the inherited list and uses `items` instead.
    #[must_use]
    pub const fn replace(items: Vec<T>) -> Self {
        Self {
            replace: true,
            items,
        }
    }

    /// Drops the inherited list: `replace` with no items.
    #[must_use]
    pub const fn omit() -> Self {
        Self::replace(Vec::new())
    }

    /// The items, in order. Once resolution is finished, the whole list.
    #[must_use]
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// `true` if this layer drops the inherited list.
    #[must_use]
    pub const fn replaces(&self) -> bool {
        self.replace
    }
}

impl<T> Default for Appended<T> {
    /// Appends nothing.
    fn default() -> Self {
        Self::append(Vec::new())
    }
}

impl<T: Clone> Merge for Appended<T> {
    fn merge_from(&mut self, overlay: &Self) {
        if overlay.replace {
            self.clone_from(overlay);
        } else {
            self.items.extend_from_slice(&overlay.items);
        }
    }
}
