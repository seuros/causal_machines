//! Randomized comparison against a naive event-set model.
//!
//! The model keeps no vectors of its own: each replica is the explicit set of
//! `(slot, counter)` events it has delivered, and a stamp's dependencies are
//! enumerated one event at a time.

extern crate std;

use std::collections::BTreeSet;
use std::vec::Vec;

use crate::{Causal, Gap, Membership, Readiness, Receipt, Release, ReplicaId, Stamp};

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
    let Some(slot) = IDS.iter().position(|id| *id == replica) else {
        panic!("model only sends from members");
    };
    slot
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

    fn check(&self, stamp: &Message) -> Readiness {
        if self.delivered.contains(&identity(stamp)) {
            Readiness::Duplicate
        } else if self.lacking(stamp).is_empty() {
            Readiness::Deliverable
        } else {
            Readiness::Missing
        }
    }

    fn receive(&mut self, stamp: &Message) -> Receipt {
        match self.check(stamp) {
            Readiness::Duplicate => Receipt::Duplicate,
            Readiness::Deliverable => {
                self.held.retain(|held| identity(held) != identity(stamp));
                self.delivered.insert(identity(stamp));
                Receipt::Delivered
            }
            Readiness::Missing => {
                if let Some(held) = self.held.iter().find(|h| identity(h) == identity(stamp)) {
                    if held == stamp {
                        Receipt::AlreadyHeld
                    } else {
                        Receipt::Conflict
                    }
                } else if self.held.len() == HOLD {
                    Receipt::Full
                } else {
                    self.held.push(*stamp);
                    Receipt::Held
                }
            }
        }
    }

    fn take(&mut self, wanted: Readiness) -> Option<Message> {
        let chosen = self
            .held
            .iter()
            .filter(|held| self.check(held) == wanted)
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

    fn absorb(&mut self, counters: &[u64; SLOTS]) {
        for (slot, count) in counters.iter().enumerate() {
            for counter in 1..=*count {
                self.delivered.insert((slot, counter));
            }
        }
    }

    fn forget(&mut self, event: Event) -> bool {
        let before = self.held.len();
        self.held.retain(|held| identity(held) != event);
        self.held.len() != before
    }

    fn gaps(&self, stamp: &Message) -> Vec<Gap> {
        let lacking = self.lacking(stamp);
        if self.check(stamp) != Readiness::Missing {
            return Vec::new();
        }
        let mut gaps = Vec::new();
        for (slot, replica) in IDS.iter().enumerate() {
            let counters = lacking.iter().filter(|event| event.0 == slot);
            let from = counters.clone().map(|event| event.1).min();
            let through = counters.map(|event| event.1).max();
            if let (Some(from), Some(through)) = (from, through) {
                gaps.push(Gap {
                    replica: *replica,
                    from,
                    through,
                });
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
}

impl World {
    fn new(seed: u64) -> Self {
        let Some(members) = Membership::from_slice(&IDS) else {
            panic!("four distinct replicas fit five slots");
        };
        let mut trackers = Vec::new();
        let mut models = Vec::new();
        for id in IDS {
            let Some(tracker) = Tracker::new(members, id, 1) else {
                panic!("every id is a member");
            };
            trackers.push(tracker);
            models.push(Naive::default());
        }
        Self {
            rng: Rng(seed),
            trackers,
            models,
            log: Vec::new(),
        }
    }

    fn send(&mut self, replica: usize) {
        let Ok(stamp) = self.trackers[replica].stamp() else {
            panic!("small counters never exhaust");
        };
        assert_eq!(self.models[replica].check(&stamp), Readiness::Deliverable);
        self.models[replica].delivered.insert(identity(&stamp));
        self.log.push(stamp);
    }

    fn receive(&mut self, replica: usize, stamp: &Message) {
        let expected = self.models[replica].check(stamp);
        assert_eq!(self.trackers[replica].check(stamp), Ok(expected));
        self.listing(replica, stamp);
        let before = self.trackers[replica].clone();
        let receipt = self.models[replica].receive(stamp);
        assert_eq!(self.trackers[replica].receive(stamp), Ok(receipt));
        if matches!(
            receipt,
            Receipt::Duplicate | Receipt::Full | Receipt::AlreadyHeld | Receipt::Conflict
        ) {
            assert_eq!(self.trackers[replica], before);
        }
    }

    fn listing(&mut self, replica: usize, stamp: &Message) {
        let blank = Gap {
            replica: 0,
            from: 0,
            through: 0,
        };
        let size = 1 + self.rng.below(3);
        let mut listed = Vec::new();
        let mut cursor = Some(0);
        while let Some(at) = cursor {
            let mut out = std::vec![blank; size];
            let Ok(page) = self.trackers[replica].missing(stamp, at, &mut out) else {
                panic!("logged stamps are well formed");
            };
            listed.extend_from_slice(&out[..page.written]);
            cursor = page.next;
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
                self.receive(replica, &stamp);
            }
            16..=17 => self.drain(replica),
            18 => {
                let donor = *self.trackers[self.rng.below(REPLICAS)].clock();
                assert_eq!(self.trackers[replica].absorb(1, &donor), Ok(()));
                self.models[replica].absorb(donor.counters());
            }
            19 if !self.log.is_empty() => {
                let event = identity(&self.log[self.rng.below(self.log.len())]);
                let dropped = self.models[replica].forget(event);
                assert_eq!(
                    self.trackers[replica].forget(IDS[event.0], event.1),
                    dropped
                );
            }
            _ => {}
        }
        self.agree(replica);
    }

    /// Replays the whole log in send order, which is a causal order, so every
    /// replica ends on the same clock with nothing left parked.
    fn settle(&mut self) {
        let log = self.log.clone();
        for replica in 0..REPLICAS {
            for stamp in &log {
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
    for seed in 1..=60_u64 {
        let mut world = World::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for _ in 0..600 {
            world.step();
        }
        world.settle();
    }
}
