//! Causal-delivery readiness for one replica.

use crate::{Membership, Refusal, ReplicaId, VersionVector};

/// The causal metadata one message travels with.
///
/// `clock` is the sender's version vector at the moment it sent: its own
/// entry numbers this message, every other entry names what the sender had
/// delivered before sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp<const N: usize> {
    /// Membership generation the stamp was issued under.
    pub generation: u64,
    /// Replica that sent the message.
    pub sender: ReplicaId,
    /// Sender's version vector, including this message.
    pub clock: VersionVector<N>,
}

impl<const N: usize> Stamp<N> {
    /// A stamp from its parts.
    #[must_use]
    pub const fn new(generation: u64, sender: ReplicaId, clock: VersionVector<N>) -> Self {
        Self {
            generation,
            sender,
            clock,
        }
    }
}

/// Whether a stamped message may be delivered now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Every dependency is delivered and this is the sender's next message.
    Deliverable,
    /// The sender's counter is already covered by the delivered clock.
    Duplicate,
    /// At least one dependency is undelivered.
    Missing,
}

/// What [`Causal::receive`] did with a stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Receipt {
    /// Delivered: the delivered clock advanced.
    Delivered,
    /// Dependencies are missing; the stamp is parked until they arrive.
    Held,
    /// The same stamp is already parked.
    AlreadyHeld,
    /// Already delivered; nothing changed.
    Duplicate,
    /// A parked stamp from the same sender with the same counter carries a
    /// different clock.
    Conflict,
    /// Dependencies are missing and every hold slot is taken; nothing changed.
    Full,
}

/// A parked stamp leaving the hold table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release<const N: usize> {
    /// Its dependencies arrived; it is now delivered.
    Deliver(Stamp<N>),
    /// The delivered clock already covers it; drop the payload.
    Obsolete(Stamp<N>),
}

/// A run of undelivered messages from one replica, inclusive at both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    /// Replica whose messages are missing.
    pub replica: ReplicaId,
    /// First missing counter.
    pub from: u64,
    /// Last missing counter.
    pub through: u64,
}

/// One bounded page of a missing-dependency listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// Gaps written to the front of the output buffer.
    pub written: usize,
    /// Cursor to pass to the next call, or `None` when the listing is done.
    pub next: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Parked<const N: usize> {
    slot: usize,
    counter: u64,
    stamp: Stamp<N>,
}

/// One replica's causal-delivery state: `N` membership slots, `P` hold slots.
///
/// Tracks the delivered clock and parks the stamps of messages that arrived
/// ahead of their dependencies. It never sees a payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Causal<const N: usize, const P: usize> {
    members: Membership<N>,
    local: usize,
    replica: ReplicaId,
    generation: u64,
    delivered: VersionVector<N>,
    parked: [Option<Parked<N>>; P],
}

impl<const N: usize, const P: usize> Causal<N, P> {
    /// A replica that has delivered nothing.
    ///
    /// Returns `None` when `local` is not in `members`.
    #[must_use]
    pub fn new(members: Membership<N>, local: ReplicaId, generation: u64) -> Option<Self> {
        Self::restore(members, local, generation, VersionVector::new())
    }

    /// A replica resuming from a persisted delivered clock.
    ///
    /// Returns `None` when `local` is not in `members` or `delivered` counts
    /// a slot outside them.
    #[must_use]
    pub fn restore(
        members: Membership<N>,
        local: ReplicaId,
        generation: u64,
        delivered: VersionVector<N>,
    ) -> Option<Self> {
        if !members.admits(&delivered) {
            return None;
        }
        Some(Self {
            local: members.slot(local)?,
            replica: local,
            members,
            generation,
            delivered,
            parked: [None; P],
        })
    }

    /// The fixed membership.
    #[must_use]
    pub const fn membership(&self) -> &Membership<N> {
        &self.members
    }

    /// The replica this state belongs to.
    #[must_use]
    pub const fn local(&self) -> ReplicaId {
        self.replica
    }

    /// Membership generation this replica accepts stamps from.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Everything delivered here so far, own sends included.
    #[must_use]
    pub const fn clock(&self) -> &VersionVector<N> {
        &self.delivered
    }

    /// Number of parked stamps.
    #[must_use]
    pub fn held(&self) -> usize {
        self.parked.iter().flatten().count()
    }

    /// Number of hold slots.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        P
    }

    /// Numbers a new local message and returns its stamp.
    ///
    /// The message counts as delivered here from this moment.
    ///
    /// # Errors
    ///
    /// [`Refusal::Exhausted`] once the local counter is `u64::MAX`.
    pub fn stamp(&mut self) -> Result<Stamp<N>, Refusal> {
        self.delivered.increment(self.local)?;
        Ok(Stamp::new(self.generation, self.replica, self.delivered))
    }

    /// Whether `stamp` could be delivered now. Changes nothing.
    ///
    /// # Errors
    ///
    /// Any [`Refusal`] but `Exhausted`: wrong generation, unknown sender, or
    /// a malformed clock.
    pub fn check(&self, stamp: &Stamp<N>) -> Result<Readiness, Refusal> {
        self.classify(stamp).map(|(_, _, readiness)| readiness)
    }

    /// Delivers `stamp` if it is ready, parks it if it is not.
    ///
    /// A message with the same sender and counter as a delivered one is
    /// dropped from the hold table when it is delivered here.
    ///
    /// # Errors
    ///
    /// As [`Causal::check`]. A refused stamp changes nothing.
    pub fn receive(&mut self, stamp: &Stamp<N>) -> Result<Receipt, Refusal> {
        let (slot, counter, readiness) = self.classify(stamp)?;
        match readiness {
            Readiness::Duplicate => Ok(Receipt::Duplicate),
            Readiness::Deliverable => {
                if let Some(index) = self.find(slot, counter) {
                    self.vacate(index);
                }
                self.delivered.set(slot, counter);
                Ok(Receipt::Delivered)
            }
            Readiness::Missing => Ok(self.park(slot, counter, stamp)),
        }
    }

    /// Takes one parked stamp that no longer needs to wait.
    ///
    /// Stamps the delivered clock already covers come out first as
    /// [`Release::Obsolete`], lowest slot then lowest counter. Then ready
    /// stamps come out as [`Release::Deliver`], lowest slot first, each
    /// advancing the delivered clock. Call until `None`.
    pub fn release(&mut self) -> Option<Release<N>> {
        if let Some(index) = self.pick(Self::covered) {
            return self
                .vacate(index)
                .map(|parked| Release::Obsolete(parked.stamp));
        }
        let index = self.pick(Self::ready)?;
        let parked = self.vacate(index)?;
        self.delivered.set(parked.slot, parked.counter);
        Some(Release::Deliver(parked.stamp))
    }

    /// Drops the parked stamp numbered `counter` from `sender`.
    ///
    /// Returns whether one was parked.
    pub fn forget(&mut self, sender: ReplicaId, counter: u64) -> bool {
        self.members
            .slot(sender)
            .and_then(|slot| self.find(slot, counter))
            .and_then(|index| self.vacate(index))
            .is_some()
    }

    /// Lists what must be delivered before `stamp`, one bounded page a call.
    ///
    /// Start with `cursor` 0 and pass each page's `next` back until it is
    /// `None`. Gaps are measured against the delivered clock; a parked stamp
    /// does not close one. A deliverable or duplicate stamp lists nothing.
    ///
    /// # Errors
    ///
    /// As [`Causal::check`].
    pub fn missing(
        &self,
        stamp: &Stamp<N>,
        cursor: usize,
        out: &mut [Gap],
    ) -> Result<Page, Refusal> {
        let (sender, _, readiness) = self.classify(stamp)?;
        let mut page = Page {
            written: 0,
            next: None,
        };
        if readiness != Readiness::Missing {
            return Ok(page);
        }
        for (slot, replica) in self.members.ids().iter().enumerate().skip(cursor) {
            let Some((from, through)) = self.gap(slot, sender, &stamp.clock) else {
                continue;
            };
            let Some(entry) = out.get_mut(page.written) else {
                page.next = Some(slot);
                break;
            };
            *entry = Gap {
                replica: *replica,
                from,
                through,
            };
            page.written += 1;
        }
        Ok(page)
    }

    /// Folds a clock learned elsewhere into the delivered clock.
    ///
    /// For state transfer: after installing a snapshot that already reflects
    /// `clock`, those messages count as delivered. Parked stamps this covers
    /// leave through [`Causal::release`] as obsolete.
    ///
    /// # Errors
    ///
    /// Generation refusals, or [`Refusal::Malformed`] when `clock` counts a
    /// slot outside the membership.
    pub fn absorb(&mut self, generation: u64, clock: &VersionVector<N>) -> Result<(), Refusal> {
        self.admit(generation)?;
        if !self.members.admits(clock) {
            return Err(Refusal::Malformed);
        }
        self.delivered.merge(clock);
        Ok(())
    }

    const fn admit(&self, generation: u64) -> Result<(), Refusal> {
        if generation < self.generation {
            Err(Refusal::StaleGeneration)
        } else if generation > self.generation {
            Err(Refusal::FutureGeneration)
        } else {
            Ok(())
        }
    }

    fn classify(&self, stamp: &Stamp<N>) -> Result<(usize, u64, Readiness), Refusal> {
        self.admit(stamp.generation)?;
        let slot = self
            .members
            .slot(stamp.sender)
            .ok_or(Refusal::UnknownMember)?;
        if !self.members.admits(&stamp.clock) {
            return Err(Refusal::Malformed);
        }
        let counter = stamp.clock.get(slot).ok_or(Refusal::Malformed)?;
        if counter == 0 {
            return Err(Refusal::Malformed);
        }
        let readiness = if counter <= self.seen(slot) {
            Readiness::Duplicate
        } else if self.blocked(slot, &stamp.clock) {
            Readiness::Missing
        } else {
            Readiness::Deliverable
        };
        Ok((slot, counter, readiness))
    }

    fn seen(&self, slot: usize) -> u64 {
        self.delivered.get(slot).unwrap_or(0)
    }

    /// The undelivered run at `slot` that a message from `sender` stamped
    /// `clock` waits for.
    fn gap(&self, slot: usize, sender: usize, clock: &VersionVector<N>) -> Option<(u64, u64)> {
        let stamped = clock.get(slot)?;
        let needed = if slot == sender {
            stamped.checked_sub(1)?
        } else {
            stamped
        };
        let seen = self.seen(slot);
        (seen < needed).then(|| (seen.saturating_add(1), needed))
    }

    fn blocked(&self, sender: usize, clock: &VersionVector<N>) -> bool {
        (0..self.members.len()).any(|slot| self.gap(slot, sender, clock).is_some())
    }

    fn covered(&self, parked: &Parked<N>) -> bool {
        parked.counter <= self.seen(parked.slot)
    }

    fn ready(&self, parked: &Parked<N>) -> bool {
        !self.blocked(parked.slot, &parked.stamp.clock)
    }

    fn find(&self, slot: usize, counter: u64) -> Option<usize> {
        self.parked.iter().position(|entry| {
            entry.is_some_and(|parked| parked.slot == slot && parked.counter == counter)
        })
    }

    /// Hold-table index of the matching stamp with the lowest slot, then the
    /// lowest counter.
    fn pick(&self, matches: impl Fn(&Self, &Parked<N>) -> bool) -> Option<usize> {
        self.parked
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.as_ref().map(|parked| (index, parked)))
            .filter(|(_, parked)| matches(self, parked))
            .min_by_key(|(_, parked)| (parked.slot, parked.counter))
            .map(|(index, _)| index)
    }

    fn park(&mut self, slot: usize, counter: u64, stamp: &Stamp<N>) -> Receipt {
        if let Some(index) = self.find(slot, counter) {
            let same = self
                .parked
                .get(index)
                .is_some_and(|entry| entry.is_some_and(|parked| parked.stamp == *stamp));
            return if same {
                Receipt::AlreadyHeld
            } else {
                Receipt::Conflict
            };
        }
        let Some(free) = self.parked.iter_mut().find(|entry| entry.is_none()) else {
            return Receipt::Full;
        };
        *free = Some(Parked {
            slot,
            counter,
            stamp: *stamp,
        });
        Receipt::Held
    }

    fn vacate(&mut self, index: usize) -> Option<Parked<N>> {
        self.parked.get_mut(index).and_then(Option::take)
    }
}
