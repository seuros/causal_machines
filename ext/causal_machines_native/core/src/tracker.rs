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

/// What an honoured [`Causal::receive`] did with a stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receipt {
    /// Delivered: the delivered clock advanced.
    Delivered,
    /// Dependencies are missing; the stamp is parked until they arrive.
    Held,
    /// An idempotent repeat: the same stamp is already parked. Nothing
    /// changed.
    AlreadyHeld,
    /// An idempotent repeat: already delivered. Nothing changed.
    Duplicate,
}

/// What an honoured [`Causal::forget`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forgetting {
    /// The parked stamp was dropped; its hold slot is free.
    Forgotten,
    /// No such stamp was parked: an idempotent repeat that changed nothing.
    NotHeld,
}

/// What an honoured [`Causal::absorb`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absorbing {
    /// The clock taught something new: the delivered clock advanced.
    Absorbed,
    /// An idempotent repeat: the delivered clock already covers the clock.
    /// Nothing changed.
    AlreadyCovered,
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
///
/// `Gap::default()` is a placeholder for pre-filling an output buffer: its
/// `from` is 0, and counters start at 1, so [`Causal::missing`] never emits it
/// and [`Gap::new`] never builds it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gap {
    replica: ReplicaId,
    from: u64,
    through: u64,
}

impl Gap {
    /// A gap from its parts, as [`Causal::missing`] emits it.
    ///
    /// Returns `None` when `from` is 0 (counters start at 1) or `from` is
    /// past `through`.
    #[must_use]
    pub const fn new(replica: ReplicaId, from: u64, through: u64) -> Option<Self> {
        if from == 0 || from > through {
            return None;
        }
        Some(Self {
            replica,
            from,
            through,
        })
    }

    /// Replica whose messages are missing.
    #[must_use]
    pub const fn replica(&self) -> ReplicaId {
        self.replica
    }

    /// First missing counter.
    #[must_use]
    pub const fn from(&self) -> u64 {
        self.from
    }

    /// Last missing counter.
    #[must_use]
    pub const fn through(&self) -> u64 {
        self.through
    }
}

/// One bounded page of a missing-dependency listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    written: usize,
    next: Option<usize>,
}

impl Page {
    /// A page as [`Causal::missing`] answers it for `N` membership slots.
    ///
    /// Each gap comes from its own slot, and a page that stops early stops at
    /// the slot it could not write, after the slots it did. So `None` when
    /// `written` exceeds `N`, `next` is not a slot below `N`, or `written`
    /// exceeds `next`.
    #[must_use]
    pub const fn new<const N: usize>(written: usize, next: Option<usize>) -> Option<Self> {
        let fits = match next {
            Some(slot) => slot < N && written <= slot,
            None => written <= N,
        };
        if fits {
            Some(Self { written, next })
        } else {
            None
        }
    }

    /// Gaps written to the front of the output buffer.
    #[must_use]
    pub const fn written(&self) -> usize {
        self.written
    }

    /// Cursor to pass to the next call, or `None` when the listing is done.
    #[must_use]
    pub const fn next(&self) -> Option<usize> {
        self.next
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Parked<const N: usize> {
    slot: usize,
    counter: u64,
    stamp: Stamp<N>,
}

impl<const N: usize> Parked<N> {
    /// Whether this is the sender's message numbered `counter` from `slot`.
    const fn numbers(&self, slot: usize, counter: u64) -> bool {
        self.slot == slot && self.counter == counter
    }
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
    /// It answers what [`Causal::receive`] would do with the stamp, short of
    /// the hold table being full.
    ///
    /// # Errors
    ///
    /// In this order: [`Refusal::StaleGeneration`] or
    /// [`Refusal::FutureGeneration`], [`Refusal::UnknownMember`] for the
    /// sender, [`Refusal::Malformed`], and [`Refusal::Conflict`] when a
    /// different stamp with the same sender and counter is parked, whatever
    /// the readiness of either.
    pub fn check(&self, stamp: &Stamp<N>) -> Result<Readiness, Refusal> {
        self.classify(stamp).map(|(_, _, readiness)| readiness)
    }

    /// Delivers `stamp` if it is ready, parks it if it is not.
    ///
    /// A stamp that is already delivered or already parked is honoured again
    /// and changes nothing. When the very stamp being delivered is also
    /// parked, because its dependencies arrived and it was received again
    /// before [`Causal::release`] was called, the parked copy leaves the hold
    /// table with it: it is the same message, delivered once.
    ///
    /// A parked stamp is never dropped or replaced for a different one. A
    /// stamp whose sender and counter match a parked stamp but whose clock
    /// does not is refused whether it would have been delivered, parked or
    /// called a duplicate: the two cannot both be the sender's message, and
    /// the tracker cannot tell which is. [`Causal::forget`] the parked one to
    /// take the other.
    ///
    /// # Errors
    ///
    /// As [`Causal::check`], [`Refusal::Conflict`] included, plus
    /// [`Refusal::Full`] when the stamp must wait and no hold slot is free.
    /// A refused stamp changes nothing.
    pub fn receive(&mut self, stamp: &Stamp<N>) -> Result<Receipt, Refusal> {
        let (slot, counter, readiness) = self.classify(stamp)?;
        match readiness {
            Readiness::Duplicate => Ok(Receipt::Duplicate),
            Readiness::Deliverable => {
                if !advance(&mut self.delivered, slot, counter) {
                    return Err(Refusal::Malformed);
                }
                // `classify` refused any other stamp: a parked one is this one.
                self.remove(slot, counter);
                Ok(Receipt::Delivered)
            }
            Readiness::Missing => self.park(slot, counter, stamp),
        }
    }

    /// Takes one parked stamp that no longer needs to wait.
    ///
    /// Stamps the delivered clock already covers come out first as
    /// [`Release::Obsolete`], lowest slot then lowest counter. Then ready
    /// stamps come out as [`Release::Deliver`], lowest slot first, each
    /// advancing the delivered clock. Call until `None`, which is a drain's
    /// "nothing is ready" and not a refusal; it changes nothing.
    pub fn release(&mut self) -> Option<Release<N>> {
        let (delivered, len) = (&mut self.delivered, self.members.len());
        if let Some((entry, parked)) = pick(&mut self.parked, |parked| {
            parked.counter <= seen(delivered, parked.slot)
        }) {
            *entry = None;
            return Some(Release::Obsolete(parked.stamp));
        }
        let (entry, parked) = pick(&mut self.parked, |parked| {
            !blocked(delivered, len, parked.slot, &parked.stamp.clock)
        })?;
        if !advance(delivered, parked.slot, parked.counter) {
            return None;
        }
        *entry = None;
        Some(Release::Deliver(parked.stamp))
    }

    /// Drops the parked stamp numbered `counter` from `sender`.
    ///
    /// This is the only call that discards a parked stamp without handing it
    /// back, and it does so by name: it is how a caller resolves a
    /// [`Refusal::Conflict`] in favour of the stamp that was refused.
    ///
    /// [`Forgetting::NotHeld`] is an idempotent repeat: no such stamp is
    /// parked, which is what the caller asked for, so nothing changes.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownMember`] when `sender` is not in the membership.
    pub fn forget(&mut self, sender: ReplicaId, counter: u64) -> Result<Forgetting, Refusal> {
        let slot = self.members.slot(sender).ok_or(Refusal::UnknownMember)?;
        Ok(if self.remove(slot, counter) {
            Forgetting::Forgotten
        } else {
            Forgetting::NotHeld
        })
    }

    /// Lists what must be delivered before `stamp`, one bounded page a call.
    ///
    /// Start with `cursor` 0 and pass each page's `next` back until it is
    /// `None`. Gaps are measured against the delivered clock; a parked stamp
    /// does not close one. A deliverable or duplicate stamp lists nothing.
    ///
    /// A cursor is a membership slot: the listing resumes at that slot. Every
    /// `next` the tracker hands out is one, and 0 always is, so a cursor past
    /// the last slot is one it never issued and is refused.
    ///
    /// The first [`Page::written`] entries of `out` are written; the rest are
    /// left untouched. Build the buffer as `[Gap::default(); K]`.
    ///
    /// # Errors
    ///
    /// As [`Causal::check`], [`Refusal::Conflict`] included, then
    /// [`Refusal::UnknownMember`] when `cursor` is not a membership slot. A
    /// refusal writes nothing to `out`.
    pub fn missing(
        &self,
        stamp: &Stamp<N>,
        cursor: usize,
        out: &mut [Gap],
    ) -> Result<Page, Refusal> {
        let (sender, _, readiness) = self.classify(stamp)?;
        if cursor >= self.members.len() {
            return Err(Refusal::UnknownMember);
        }
        let mut page = Page {
            written: 0,
            next: None,
        };
        if readiness != Readiness::Missing {
            return Ok(page);
        }
        for (slot, replica) in self.members.ids().iter().enumerate().skip(cursor) {
            let Some((from, through)) = gap(&self.delivered, slot, sender, &stamp.clock) else {
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
    /// [`Absorbing::AlreadyCovered`] is an idempotent repeat: the delivered
    /// clock already dominates `clock`, so nothing changes.
    ///
    /// # Errors
    ///
    /// Generation refusals, or [`Refusal::Malformed`] when `clock` counts a
    /// slot outside the membership.
    pub fn absorb(
        &mut self,
        generation: u64,
        clock: &VersionVector<N>,
    ) -> Result<Absorbing, Refusal> {
        self.admit(generation)?;
        if !self.members.admits(clock) {
            return Err(Refusal::Malformed);
        }
        if self.delivered.dominates(clock) {
            return Ok(Absorbing::AlreadyCovered);
        }
        self.delivered.merge(clock);
        Ok(Absorbing::Absorbed)
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
        let Some(counter) = stamp.clock.get(slot) else {
            debug_assert!(false, "a sender's slot is a membership slot");
            return Err(Refusal::Malformed);
        };
        if counter == 0 {
            return Err(Refusal::Malformed);
        }
        if self.conflicts(slot, counter, stamp) {
            return Err(Refusal::Conflict);
        }
        let readiness = if counter <= seen(&self.delivered, slot) {
            Readiness::Duplicate
        } else if blocked(&self.delivered, self.members.len(), slot, &stamp.clock) {
            Readiness::Missing
        } else {
            Readiness::Deliverable
        };
        Ok((slot, counter, readiness))
    }

    /// Whether a parked stamp numbers the same message as `stamp` but is not
    /// `stamp`. At most one stamp is parked per sender and counter.
    fn conflicts(&self, slot: usize, counter: u64, stamp: &Stamp<N>) -> bool {
        self.parked
            .iter()
            .flatten()
            .any(|parked| parked.numbers(slot, counter) && parked.stamp != *stamp)
    }

    fn holds(&self, slot: usize, counter: u64) -> bool {
        self.parked
            .iter()
            .flatten()
            .any(|parked| parked.numbers(slot, counter))
    }

    /// Drops the parked stamp numbered `counter` from `slot`, if there is
    /// one, and says whether there was.
    fn remove(&mut self, slot: usize, counter: u64) -> bool {
        let Some(entry) = self
            .parked
            .iter_mut()
            .find(|entry| entry.is_some_and(|parked| parked.numbers(slot, counter)))
        else {
            return false;
        };
        *entry = None;
        true
    }

    fn park(&mut self, slot: usize, counter: u64, stamp: &Stamp<N>) -> Result<Receipt, Refusal> {
        // `classify` refused any other stamp: a parked one is this one.
        if self.holds(slot, counter) {
            return Ok(Receipt::AlreadyHeld);
        }
        let free = self
            .parked
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or(Refusal::Full)?;
        *free = Some(Parked {
            slot,
            counter,
            stamp: *stamp,
        });
        Ok(Receipt::Held)
    }
}

/// What `delivered` has seen from `slot`.
fn seen<const N: usize>(delivered: &VersionVector<N>, slot: usize) -> u64 {
    delivered.get(slot).unwrap_or_else(|| {
        debug_assert!(false, "only membership slots are read");
        0
    })
}

/// Records `counter` as the last message `delivered` has seen from `slot`.
///
/// Callers pass a sender's membership slot, so this always succeeds; `false`
/// is the arm that invariant rules out, with nothing changed.
fn advance<const N: usize>(delivered: &mut VersionVector<N>, slot: usize, counter: u64) -> bool {
    let Some(seen) = delivered.counter_mut(slot) else {
        debug_assert!(false, "a sender's slot is a membership slot");
        return false;
    };
    *seen = counter;
    true
}

/// The undelivered run at `slot` that a message from `sender` stamped `clock`
/// waits for, against `delivered`.
fn gap<const N: usize>(
    delivered: &VersionVector<N>,
    slot: usize,
    sender: usize,
    clock: &VersionVector<N>,
) -> Option<(u64, u64)> {
    let stamped = clock.get(slot)?;
    let needed = if slot == sender {
        stamped.checked_sub(1)?
    } else {
        stamped
    };
    let seen = seen(delivered, slot);
    (seen < needed).then(|| (seen.saturating_add(1), needed))
}

fn blocked<const N: usize>(
    delivered: &VersionVector<N>,
    len: usize,
    sender: usize,
    clock: &VersionVector<N>,
) -> bool {
    (0..len).any(|slot| gap(delivered, slot, sender, clock).is_some())
}

/// The hold-table entry of the matching stamp with the lowest slot, then the
/// lowest counter, with a copy of the stamp.
fn pick<const N: usize>(
    parked: &mut [Option<Parked<N>>],
    matches: impl Fn(&Parked<N>) -> bool,
) -> Option<(&mut Option<Parked<N>>, Parked<N>)> {
    parked
        .iter_mut()
        .filter_map(|entry| {
            let held = (*entry)?;
            matches(&held).then_some((entry, held))
        })
        .min_by_key(|(_, held)| (held.slot, held.counter))
}
