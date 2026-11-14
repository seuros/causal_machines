//! Randomized comparison against a naive event-set model.
//!
//! The model keeps no vectors of its own: each replica is the explicit set of
//! `(slot, counter)` events it has delivered, and a stamp's dependencies are
//! enumerated one event at a time.

extern crate std;

use std::collections::BTreeSet;
use std::vec::Vec;

use crate::{
    Absorbing, Causal, Forgetting, Gap, Membership, Readiness, Receipt, Refusal, Release,
    ReplicaId, Stamp, VersionVector,
};

const SLOTS: usize = 5;
const REPLICAS: usize = 4;
const HOLD: usize = 3;
const IDS: [ReplicaId; REPLICAS] = [11, 22, 33, 44];

type Event = (usize, u64);
type Tracker = Causal<SLOTS, HOLD>;
type Message = Stamp<SLOTS>;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).unwrap_or(u64::MAX).max(1);
        usize::try_from(self.next() % bound).unwrap_or(0)
    }
}

fn slot_of(replica: ReplicaId) -> usize {
    IDS.iter()
        .position(|id| *id == replica)
        .expect("model only sends from members")
}

fn identity(stamp: &Message) -> Event {
    let slot = slot_of(stamp.sender);
    (slot, stamp.clock.counters()[slot])
}

/// Every event the message needs delivered first.
fn dependencies(stamp: &Message) -> Vec<Event> {
    let own = identity(stamp);
    let mut needed = Vec::new();
    for (slot, count) in stamp.clock.counters().iter().enumerate() {
        for counter in 1..=*count {
            if (slot, counter) != own {
                needed.push((slot, counter));
            }
        }
    }
    needed
}

#[derive(Default)]
struct Naive {
    delivered: BTreeSet<Event>,
    held: Vec<Message>,
}

impl Naive {
    fn lacking(&self, stamp: &Message) -> Vec<Event> {
        let mut lacking = dependencies(stamp);
        lacking.retain(|event| !self.delivered.contains(event));
        lacking
    }

    /// A held message numbered like `stamp` that is not `stamp`.
    fn conflict(&self, stamp: &Message) -> bool {
        let mut same = self.held.iter().filter(|h| identity(h) == identity(stamp));
        same.any(|held| held != stamp)
    }

    fn check(&self, stamp: &Message) -> Result<Readiness, Refusal> {
        if self.conflict(stamp) {
            Err(Refusal::Conflict)
        } else {
            Ok(self.readiness(stamp))
        }
    }

    fn readiness(&self, stamp: &Message) -> Readiness {
        if self.delivered.contains(&identity(stamp)) {
            Readiness::Duplicate
        } else if self.lacking(stamp).is_empty() {
            Readiness::Deliverable
        } else {
            Readiness::Missing
        }
    }

    fn receive(&mut self, stamp: &Message) -> Result<Receipt, Refusal> {
        match self.check(stamp)? {
            Readiness::Duplicate => Ok(Receipt::Duplicate),
            Readiness::Deliverable => {
                // Only the message itself is left to drop from the hold.
                self.held.retain(|held| held != stamp);
                self.delivered.insert(identity(stamp));
                Ok(Receipt::Delivered)
            }
            Readiness::Missing => {
                if self.held.contains(stamp) {
                    Ok(Receipt::AlreadyHeld)
                } else if self.held.len() == HOLD {
                    Err(Refusal::Full)
                } else {
                    self.held.push(*stamp);
                    Ok(Receipt::Held)
                }
            }
        }
    }

    fn take(&mut self, wanted: Readiness) -> Option<Message> {
        let chosen = self
            .held
            .iter()
            .filter(|held| self.readiness(held) == wanted)
            .map(identity)
            .min()?;
        let index = self.held.iter().position(|h| identity(h) == chosen)?;
        Some(self.held.remove(index))
    }

    fn release(&mut self) -> Option<Release<SLOTS>> {
        if let Some(stamp) = self.take(Readiness::Duplicate) {
            return Some(Release::Obsolete(stamp));
        }
        let stamp = self.take(Readiness::Deliverable)?;
        self.delivered.insert(identity(&stamp));
        Some(Release::Deliver(stamp))
    }

    /// Whether the clock taught anything new.
    fn absorb(&mut self, counters: &[u64; SLOTS]) -> bool {
        let before = self.delivered.len();
        for (slot, count) in counters.iter().enumerate() {
            for counter in 1..=*count {
                self.delivered.insert((slot, counter));
            }
        }
        self.delivered.len() != before
    }

    fn forget(&mut self, event: Event) -> bool {
        let before = self.held.len();
        self.held.retain(|held| identity(held) != event);
        self.held.len() != before
    }

    fn gaps(&self, stamp: &Message) -> Vec<Gap> {
        let lacking = self.lacking(stamp);
        if self.readiness(stamp) != Readiness::Missing {
            return Vec::new();
        }
        let mut gaps = Vec::new();
        for (slot, replica) in IDS.iter().enumerate() {
            let counters = lacking.iter().filter(|event| event.0 == slot);
            let from = counters.clone().map(|event| event.1).min();
            let through = counters.map(|event| event.1).max();
            if let (Some(from), Some(through)) = (from, through) {
                gaps.extend(Gap::new(*replica, from, through));
            }
        }
        gaps
    }

    fn count(&self, slot: usize) -> usize {
        self.delivered
            .iter()
            .filter(|event| event.0 == slot)
            .count()
    }
}

struct World {
    rng: Rng,
    trackers: Vec<Tracker>,
    models: Vec<Naive>,
    log: Vec<Message>,
    /// Conflicts refused, by what the stamp would otherwise have been:
    /// deliverable, missing, duplicate.
    conflicts: [usize; 3],
}

impl World {
    fn new(seed: u64) -> Self {
        let members = Membership::from_slice(&IDS).expect("four distinct replicas fit five slots");
        let mut trackers = Vec::new();
        let mut models = Vec::new();
        for id in IDS {
            trackers.push(Tracker::new(members, id, 1).expect("every id is a member"));
            models.push(Naive::default());
        }
        Self {
            rng: Rng(seed),
            trackers,
            models,
            log: Vec::new(),
            conflicts: [0; 3],
        }
    }

    fn send(&mut self, replica: usize) {
        let stamp = self.trackers[replica]
            .stamp()
            .expect("small counters never exhaust");
        assert_eq!(
            self.models[replica].check(&stamp),
            Ok(Readiness::Deliverable)
        );
        self.models[replica].delivered.insert(identity(&stamp));
        self.log.push(stamp);
    }

    fn receive(&mut self, replica: usize, stamp: &Message) {
        let expected = self.models[replica].check(stamp);
        assert_eq!(self.trackers[replica].check(stamp), expected);
        if expected == Err(Refusal::Conflict) {
            let kind = match self.models[replica].readiness(stamp) {
                Readiness::Deliverable => 0,
                Readiness::Missing => 1,
                Readiness::Duplicate => 2,
            };
            self.conflicts[kind] += 1;
        }
        self.listing(replica, stamp);
        let before = self.trackers[replica].clone();
        let receipt = self.models[replica].receive(stamp);
        assert_eq!(self.trackers[replica].receive(stamp), receipt);
        // A refusal or an idempotent repeat leaves the whole value alone.
        if !matches!(receipt, Ok(Receipt::Delivered | Receipt::Held)) {
            assert_eq!(self.trackers[replica], before);
        }
    }

    fn listing(&mut self, replica: usize, stamp: &Message) {
        let size = 1 + self.rng.below(3);
        // A cursor past the membership was never issued: refused once the
        // stamp itself is judged, and nothing is written.
        let unissued = REPLICAS + self.rng.below(SLOTS);
        let mut out = std::vec![Gap::default(); size];
        let expected = self.models[replica]
            .check(stamp)
            .and(Err(Refusal::UnknownMember));
        let page = self.trackers[replica].missing(stamp, unissued, &mut out);
        assert_eq!(page, expected);
        assert!(out.iter().all(|gap| *gap == Gap::default()));
        let mut listed = Vec::new();
        let mut cursor = Some(0);
        while let Some(at) = cursor {
            let mut out = std::vec![Gap::default(); size];
            let page = self.trackers[replica].missing(stamp, at, &mut out);
            let Ok(page) = page else {
                assert_eq!(
                    page.map(|_| ()),
                    self.models[replica].check(stamp).map(|_| ())
                );
                return;
            };
            listed.extend_from_slice(&out[..page.written()]);
            cursor = page.next();
        }
        assert_eq!(listed, self.models[replica].gaps(stamp));
    }

    fn drain(&mut self, replica: usize) {
        loop {
            let expected = self.models[replica].release();
            assert_eq!(self.trackers[replica].release(), expected);
            if expected.is_none() {
                break;
            }
        }
    }

    fn agree(&self, replica: usize) {
        let tracker = &self.trackers[replica];
        let model = &self.models[replica];
        assert_eq!(tracker.held(), model.held.len());
        for (slot, counter) in tracker.clock().counters().iter().enumerate() {
            assert_eq!(usize::try_from(*counter), Ok(model.count(slot)));
        }
    }

    fn step(&mut self) {
        let replica = self.rng.below(REPLICAS);
        match self.rng.below(20) {
            0..=4 => self.send(replica),
            5..=15 if !self.log.is_empty() => {
                let stamp = self.log[self.rng.below(self.log.len())];
                // One arrival in four is a conflicting copy: of a message
                // parked here when there is one, so that it arrives after
                // some of the gap has filled, else of any message.
                let held = &self.models[replica].held;
                let stamp = match (self.rng.below(4), held.len()) {
                    (0, 0) => self.forged(&stamp),
                    (0, _) => {
                        let parked = held[self.rng.below(held.len())];
                        self.forged(&parked)
                    }
                    _ => stamp,
                };
                self.receive(replica, &stamp);
            }
            16..=17 => self.drain(replica),
            18 => {
                let donor = *self.trackers[self.rng.below(REPLICAS)].clock();
                let before = self.trackers[replica].clone();
                let answer = self.trackers[replica].absorb(1, &donor);
                if self.models[replica].absorb(donor.counters()) {
                    assert_eq!(answer, Ok(Absorbing::Absorbed));
                } else {
                    assert_eq!(answer, Ok(Absorbing::AlreadyCovered));
                    assert_eq!(self.trackers[replica], before);
                }
            }
            19 if !self.log.is_empty() => {
                let event = identity(&self.log[self.rng.below(self.log.len())]);
                let dropped = self.models[replica].forget(event);
                let expected = if dropped {
                    Forgetting::Forgotten
                } else {
                    Forgetting::NotHeld
                };
                let before = self.trackers[replica].clone();
                assert_eq!(
                    self.trackers[replica].forget(IDS[event.0], event.1),
                    Ok(expected)
                );
                assert_eq!(
                    self.trackers[replica].forget(99, event.1),
                    Err(Refusal::UnknownMember)
                );
                if !dropped {
                    assert_eq!(self.trackers[replica], before);
                }
            }
            _ => {}
        }
        self.agree(replica);
    }

    /// The same sender and counter as `stamp` with fewer dependencies, so it
    /// conflicts with `stamp` and is ready no later than it: a copy that can
    /// arrive deliverable while the genuine message is still parked. `stamp`
    /// itself when there is nothing to take away.
    fn forged(&mut self, stamp: &Message) -> Message {
        let sender = slot_of(stamp.sender);
        let mut counters = *stamp.clock.counters();
        // Half the forgeries depend on nobody else; the rest need one
        // message fewer from one other replica.
        let spared = self.rng.below(SLOTS);
        let all = self.rng.below(2) == 0;
        for (slot, counter) in counters.iter_mut().enumerate() {
            if slot == sender {
                continue;
            }
            if all {
                *counter = 0;
            } else if slot == spared {
                *counter = counter.saturating_sub(1);
            }
        }
        Message::new(
            stamp.generation,
            stamp.sender,
            VersionVector::from_counters(counters),
        )
    }

    /// Replays the whole log in send order, which is a causal order, so every
    /// replica ends on the same clock with nothing left parked. Draining
    /// first lets a parked forgery, which waits for less than the genuine
    /// message, leave before that message arrives and would be refused.
    fn settle(&mut self) {
        let log = self.log.clone();
        for replica in 0..REPLICAS {
            for stamp in &log {
                self.drain(replica);
                self.receive(replica, stamp);
            }
            self.drain(replica);
            self.agree(replica);
            assert_eq!(self.trackers[replica].held(), 0);
            assert_eq!(self.trackers[replica].clock(), self.trackers[0].clock());
        }
        let total: u64 = self.trackers[0].clock().counters().iter().sum();
        assert_eq!(usize::try_from(total), Ok(log.len()));
    }
}

#[test]
fn random_schedules_match_the_event_set_model() {
    let mut conflicts = [0; 3];
    for seed in 1..=60_u64 {
        let mut world = World::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for _ in 0..600 {
            world.step();
        }
        world.settle();
        for (total, seen) in conflicts.iter_mut().zip(world.conflicts) {
            *total += seen;
        }
    }
    // Conflicting copies arrived deliverable, missing and already covered.
    assert!(
        conflicts.iter().all(|&n| n > 20),
        "conflicts: {conflicts:?}"
    );
}
