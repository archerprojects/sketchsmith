//! SketchSmith — journaled entity arena | v1.0.0 | Thursday 12 November 2026
//!
//! An ordered map from key to entity that can record a *journal*: the first
//! time an entity is touched (inserted, mutated, or removed) during a
//! transaction, its prior state is saved. Committing turns the journal into a
//! list of `(key, before, after)` changes; undo writes the befores back, redo
//! the afters. This is copy-on-write undo — only touched
//! entities are copied, so the cost of undo scales with the size of the edit,
//! not the size of the model.
//!
//! Entities are held shared ([`Arc`]): a saved state, a change's before and
//! after, and a cloned arena point at the same entity as the live map. An
//! entity is copied only when it is changed while something else still points
//! at it, so an undo step and a copy of a context cost pointers, not the
//! geometry.
//!
//! The map is a `BTreeMap`, so iteration is in key (creation) order and every
//! operation that iterates entities is deterministic.

use std::collections::BTreeMap;
use std::ops::{Index, IndexMut};
use std::sync::Arc;

use crate::ids::Key;

/// One entity's state before and after a transaction (`None` = absent).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Change<K, T> {
    pub(crate) key: K,
    pub(crate) before: Option<T>,
    pub(crate) after: Option<T>,
}

/// A change recorded by an [`Arena`]: its states are shared with the arena.
pub(crate) type SharedChange<K, T> = Change<K, Arc<T>>;

#[derive(Debug, Clone)]
pub(crate) struct Arena<K: Key, T> {
    map: BTreeMap<K, Arc<T>>,
    next: u64,
    journal: Option<BTreeMap<K, Option<Arc<T>>>>,
}

impl<K: Key, T> Default for Arena<K, T> {
    fn default() -> Self {
        Self {
            map: BTreeMap::new(),
            next: 1,
            journal: None,
        }
    }
}

impl<K: Key, T: PartialEq> PartialEq for Arena<K, T> {
    /// Content equality. The key counter and journal are bookkeeping and are ignored.
    fn eq(&self, other: &Self) -> bool {
        self.map.len() == other.map.len()
            && self
                .map
                .iter()
                .zip(other.map.iter())
                .all(|((ka, a), (kb, b))| ka == kb && same(a, b))
    }
}

/// Whether two shared states are equal: the same entity, or equal contents.
fn same<T: PartialEq>(a: &Arc<T>, b: &Arc<T>) -> bool {
    Arc::ptr_eq(a, b) || **a == **b
}

/// [`same`] for states that may be absent.
fn same_opt<T: PartialEq>(a: Option<&Arc<T>>, b: Option<&Arc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => same(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl<K: Key, T: Clone + PartialEq> Arena<K, T> {
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    pub(crate) fn get(&self, k: K) -> Option<&T> {
        self.map.get(&k).map(|v| &**v)
    }

    pub(crate) fn contains_key(&self, k: K) -> bool {
        self.map.contains_key(&k)
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.map.keys().copied()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (K, &T)> + '_ {
        self.map.iter().map(|(k, v)| (*k, &**v))
    }

    /// Saves `k`'s current state in the journal if this is its first touch.
    fn touch(&mut self, k: K) {
        if let Some(journal) = &mut self.journal
            && !journal.contains_key(&k)
        {
            journal.insert(k, self.map.get(&k).cloned());
        }
    }

    pub(crate) fn insert(&mut self, value: T) -> K {
        let k = K::from_raw(self.next);
        self.next += 1;
        self.touch(k);
        self.map.insert(k, Arc::new(value));
        k
    }

    pub(crate) fn remove(&mut self, k: K) -> Option<T> {
        if !self.map.contains_key(&k) {
            return None;
        }
        self.touch(k);
        self.map.remove(&k).map(Arc::unwrap_or_clone)
    }

    /// The entity to change: copied first if anything else (a saved state,
    /// an undo step, a cloned arena) still points at it.
    pub(crate) fn get_mut(&mut self, k: K) -> Option<&mut T> {
        if self.map.contains_key(&k) {
            self.touch(k);
        }
        self.map.get_mut(&k).map(Arc::make_mut)
    }

    /// Starts recording. Any journal already open is discarded.
    pub(crate) fn begin_journal(&mut self) {
        self.journal = Some(BTreeMap::new());
    }

    pub(crate) fn journal_open(&self) -> bool {
        self.journal.is_some()
    }

    /// Whether anything has been touched since the journal opened.
    pub(crate) fn journal_touched(&self) -> bool {
        self.journal.as_ref().is_some_and(|j| !j.is_empty())
    }

    /// Whether an entity touched since the journal opened now differs from
    /// its saved state in a way `same` does not excuse (an insert or a
    /// removal always counts).
    pub(crate) fn journal_changed_beyond(&self, same: impl Fn(&T, &T) -> bool) -> bool {
        self.journal.as_ref().is_some_and(|j| {
            j.iter().any(|(k, before)| match (before, self.map.get(k)) {
                (Some(b), Some(a)) => !Arc::ptr_eq(b, a) && !same(b, a),
                (None, None) => false,
                _ => true,
            })
        })
    }

    /// `k`'s saved state in the open journal, if it was touched and existed.
    pub(crate) fn journal_before(&self, k: K) -> Option<&T> {
        self.journal.as_ref()?.get(&k)?.as_deref()
    }

    /// The entities changed since the journal opened: each one's saved state
    /// and its state now (`None`: absent). Touched but unchanged ones are
    /// left out.
    pub(crate) fn journal_changes(&self) -> impl Iterator<Item = (Option<&T>, Option<&T>)> + '_ {
        self.journal.iter().flatten().filter_map(|(k, before)| {
            let now = self.map.get(k);
            (!same_opt(before.as_ref(), now)).then(|| (before.as_deref(), now.map(|v| &**v)))
        })
    }

    /// Stops recording and returns the changes, dropping entities whose state
    /// ended up unchanged. The changes share their states with the arena.
    pub(crate) fn take_journal(&mut self) -> Vec<SharedChange<K, T>> {
        let Some(journal) = self.journal.take() else {
            return Vec::new();
        };
        journal
            .into_iter()
            .filter_map(|(key, before)| {
                let after = self.map.get(&key).cloned();
                (!same_opt(before.as_ref(), after.as_ref())).then_some(Change {
                    key,
                    before,
                    after,
                })
            })
            .collect()
    }

    /// Stops recording and restores every touched entity to its saved state.
    pub(crate) fn rollback_journal(&mut self) {
        if let Some(journal) = self.journal.take() {
            for (k, before) in journal {
                self.put(k, before);
            }
        }
    }

    /// Writes one side of each change back: `before` for undo, `after` for redo.
    pub(crate) fn apply(&mut self, changes: &[SharedChange<K, T>], undo: bool) {
        for c in changes {
            let image = if undo { &c.before } else { &c.after };
            self.put(c.key, image.clone());
        }
    }

    fn put(&mut self, k: K, value: Option<Arc<T>>) {
        match value {
            Some(v) => {
                self.map.insert(k, v);
            }
            None => {
                self.map.remove(&k);
            }
        }
    }
}

impl<K: Key, T> Index<K> for Arena<K, T> {
    type Output = T;

    /// Panics on a missing key. Used only where the key is an internal invariant
    /// (an id just read from a live entity).
    fn index(&self, k: K) -> &T {
        match self.map.get(&k) {
            Some(v) => v,
            None => panic!("internal invariant: entity {k:?} must exist"),
        }
    }
}

impl<K: Key, T: Clone + PartialEq> IndexMut<K> for Arena<K, T> {
    fn index_mut(&mut self, k: K) -> &mut T {
        self.touch(k);
        match self.map.get_mut(&k) {
            Some(v) => Arc::make_mut(v),
            None => panic!("internal invariant: entity {k:?} must exist"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::VertexId;

    #[test]
    fn journal_records_first_touch_only_and_round_trips() {
        let mut a: Arena<VertexId, i32> = Arena::default();
        let k1 = a.insert(1);
        let k2 = a.insert(2);
        a.begin_journal();
        a[k1] = 10;
        a[k1] = 11; // second touch: before-image stays 1
        a.remove(k2);
        let k3 = a.insert(3);
        let changes = a.take_journal();
        assert_eq!(changes.len(), 3);

        let after = a.clone();
        a.apply(&changes, true);
        assert_eq!(a.get(k1), Some(&1));
        assert_eq!(a.get(k2), Some(&2));
        assert_eq!(a.get(k3), None);
        a.apply(&changes, false);
        assert_eq!(a, after);
    }

    #[test]
    fn unchanged_entities_are_dropped_and_rollback_restores() {
        let mut a: Arena<VertexId, i32> = Arena::default();
        let k = a.insert(5);
        a.begin_journal();
        a[k] = 5;
        assert!(a.take_journal().is_empty());
        a.begin_journal();
        a[k] = 9;
        a.insert(7);
        a.rollback_journal();
        assert_eq!(a.len(), 1);
        assert_eq!(a.get(k), Some(&5));
    }

    #[test]
    fn changes_share_states_with_the_arena() {
        let mut a: Arena<VertexId, Vec<i32>> = Arena::default();
        a.begin_journal();
        let k = a.insert(vec![1, 2, 3]);
        let changes = a.take_journal();
        let after = changes[0].after.as_ref().expect("inserted");
        // The undo step points at the live entity: no copy was made.
        assert!(Arc::ptr_eq(after, &a.map[&k]));
        assert_eq!(Arc::strong_count(after), 2);

        // A clone of the arena shares every entity too.
        let copy = a.clone();
        assert!(Arc::ptr_eq(&copy.map[&k], &a.map[&k]));
        drop(copy);

        // Changing it copies it once; the undo step keeps the old state.
        a.begin_journal();
        a[k].push(4);
        a[k].push(5);
        assert_eq!(changes[0].after.as_deref(), Some(&vec![1, 2, 3]));
        assert_eq!(a.get(k), Some(&vec![1, 2, 3, 4, 5]));
        let second = a.take_journal();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].before.as_deref(), Some(&vec![1, 2, 3]));
        assert!(Arc::ptr_eq(second[0].before.as_ref().unwrap(), after));

        // Undo and redo put the shared states back.
        a.apply(&second, true);
        assert!(Arc::ptr_eq(&a.map[&k], after));
        a.apply(&second, false);
        assert_eq!(a.get(k), Some(&vec![1, 2, 3, 4, 5]));
    }

    #[test]
    fn touching_without_change_records_nothing() {
        let mut a: Arena<VertexId, Vec<i32>> = Arena::default();
        let k = a.insert(vec![7]);
        let held = a.clone();
        a.begin_journal();
        let _ = a.get_mut(k);
        // The entity was copied for writing but is unchanged: no change.
        assert!(a.take_journal().is_empty());
        assert_eq!(a, held);
    }

    #[test]
    fn keys_are_never_reused() {
        let mut a: Arena<VertexId, i32> = Arena::default();
        let k = a.insert(1);
        a.remove(k);
        assert_ne!(a.insert(2), k);
    }
}
