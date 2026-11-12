//! Fixed memberships and the version vectors counted over them.

use crate::{Refusal, ReplicaId};

/// How two version vectors relate in the causal partial order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Causality {
    /// Both have seen exactly the same events.
    Equal,
    /// The left side has seen a strict subset of the right side's events.
    Before,
    /// The left side has seen a strict superset of the right side's events.
    After,
    /// Each side has seen an event the other has not.
    Concurrent,
}

/// Up to `N` distinct replicas, each bound to one slot for good.
///
/// Membership never changes. A different set of replicas is a different
/// membership, and the caller gives it a new generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Membership<const N: usize> {
    ids: [ReplicaId; N],
    len: usize,
}

impl<const N: usize> Membership<N> {
    /// Binds each replica in `ids` to the slot of its position.
    ///
    /// Returns `None` when `ids` holds more than `N` replicas or names one
    /// twice.
    #[must_use]
    pub fn from_slice(ids: &[ReplicaId]) -> Option<Self> {
        if ids.len() > N {
            return None;
        }
        let mut bound = [0; N];
        for (index, id) in ids.iter().enumerate() {
            if ids.iter().take(index).any(|earlier| earlier == id) {
                return None;
            }
            *bound.get_mut(index)? = *id;
        }
        Some(Self {
            ids: bound,
            len: ids.len(),
        })
    }

    /// Number of member replicas.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no replica is a member.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Member replicas in slot order.
    #[must_use]
    pub fn ids(&self) -> &[ReplicaId] {
        self.ids.get(..self.len).unwrap_or(&[])
    }

    /// Slot bound to `replica`.
    #[must_use]
    pub fn slot(&self, replica: ReplicaId) -> Option<usize> {
        self.ids().iter().position(|id| *id == replica)
    }

    /// Replica bound to `slot`.
    #[must_use]
    pub fn replica(&self, slot: usize) -> Option<ReplicaId> {
        self.ids().get(slot).copied()
    }

    /// Whether `clock` counts nothing outside this membership's slots.
    #[must_use]
    pub fn admits(&self, clock: &VersionVector<N>) -> bool {
        clock
            .counters
            .iter()
            .skip(self.len)
            .all(|count| *count == 0)
    }
}

/// One event counter per membership slot.
///
/// Entry `k` is the number of events from slot `k`'s replica this vector has
/// seen. Vectors are only comparable within one membership generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionVector<const N: usize> {
    counters: [u64; N],
}

impl<const N: usize> Default for VersionVector<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> VersionVector<N> {
    /// A vector that has seen nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self { counters: [0; N] }
    }

    /// A vector holding exactly `counters`, in slot order.
    #[must_use]
    pub const fn from_counters(counters: [u64; N]) -> Self {
        Self { counters }
    }

    /// Counters in slot order.
    #[must_use]
    pub const fn counters(&self) -> &[u64; N] {
        &self.counters
    }

    /// Counter at `slot`, or `None` past the last slot.
    #[must_use]
    pub fn get(&self, slot: usize) -> Option<u64> {
        self.counters.get(slot).copied()
    }

    /// Counts one more event at `slot` and returns the new counter.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownMember`] past the last slot, [`Refusal::Exhausted`]
    /// at `u64::MAX`. The vector is unchanged in both cases.
    pub fn increment(&mut self, slot: usize) -> Result<u64, Refusal> {
        let counter = self.counters.get_mut(slot).ok_or(Refusal::UnknownMember)?;
        let next = counter.checked_add(1).ok_or(Refusal::Exhausted)?;
        *counter = next;
        Ok(next)
    }

    /// Where this vector sits relative to `other` in the causal order.
    #[must_use]
    pub fn compare(&self, other: &Self) -> Causality {
        let mut behind = false;
        let mut ahead = false;
        for (mine, theirs) in self.counters.iter().zip(&other.counters) {
            behind |= mine < theirs;
            ahead |= mine > theirs;
        }
        match (behind, ahead) {
            (false, false) => Causality::Equal,
            (true, false) => Causality::Before,
            (false, true) => Causality::After,
            (true, true) => Causality::Concurrent,
        }
    }

    /// Whether this vector has seen everything `other` has (equal counts).
    #[must_use]
    pub fn dominates(&self, other: &Self) -> bool {
        matches!(self.compare(other), Causality::Equal | Causality::After)
    }

    /// Whether each vector has seen an event the other has not.
    #[must_use]
    pub fn concurrent(&self, other: &Self) -> bool {
        self.compare(other) == Causality::Concurrent
    }

    /// Raises every counter to the larger of the two sides.
    ///
    /// Commutative, associative and idempotent; with a fixed `N` it cannot
    /// run out of room.
    pub fn merge(&mut self, other: &Self) {
        for (mine, theirs) in self.counters.iter_mut().zip(&other.counters) {
            *mine = (*mine).max(*theirs);
        }
    }

    /// The merge of both vectors, leaving each untouched.
    #[must_use]
    pub fn merged(mut self, other: &Self) -> Self {
        self.merge(other);
        self
    }

    pub(crate) fn set(&mut self, slot: usize, value: u64) {
        if let Some(counter) = self.counters.get_mut(slot) {
            *counter = value;
        }
    }
}
