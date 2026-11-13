//! A stamp that conflicts with a parked one is refused on every path.

use super::*;

/// The reviewed sequence: `Causal<3, 1>`, members `[1, 2, 3]`, local 3.
#[test]
fn a_conflicting_stamp_is_refused_after_the_gap_fills() {
    let Some(members) = Membership::<3>::from_slice(&[1, 2, 3]) else {
        panic!("three distinct replicas fit three slots");
    };
    let Some(mut node) = Causal::<3, 1>::new(members, 3, 1) else {
        panic!("local replica is a member");
    };
    let from = |counters| Stamp::new(1, 1, VersionVector::<3>::from_counters(counters));
    let parked = from([2, 1, 0]);
    assert_eq!(node.receive(&parked), Ok(Receipt::Held));
    assert_eq!(node.receive(&from([1, 0, 0])), Ok(Receipt::Delivered));
    let before = node.clone();
    assert_eq!(node.receive(&from([2, 0, 0])), Err(Refusal::Conflict));
    assert_eq!(node, before);
    assert_eq!(node.held(), 1);
    assert_eq!(node.clock().counters(), &[1, 0, 0]);
    // The parked stamp is still the one that will be delivered.
    assert_eq!(
        node.absorb(1, &VersionVector::from_counters([0, 1, 0])),
        Ok(())
    );
    assert_eq!(node.release(), Some(Release::Deliver(parked)));
}

#[test]
fn check_and_missing_refuse_what_receive_refuses() {
    let mut c = node(C);
    let parked = stamp(A, [2, 1, 0, 0]);
    assert_eq!(c.receive(&parked), Ok(Receipt::Held));
    assert_eq!(c.receive(&stamp(A, [1, 0, 0, 0])), Ok(Receipt::Delivered));
    let before = c.clone();
    // Deliverable, were it not for the parked stamp.
    let ready = stamp(A, [2, 0, 0, 0]);
    // Missing dependencies of its own, and a hold slot is free.
    let waiting = stamp(A, [2, 0, 1, 0]);
    for other in [ready, waiting] {
        assert_eq!(c.check(&other), Err(Refusal::Conflict));
        assert_eq!(c.receive(&other), Err(Refusal::Conflict));
        let mut out = [Gap::default(); 4];
        assert_eq!(c.missing(&other, 0, &mut out), Err(Refusal::Conflict));
        assert_eq!(out, [Gap::default(); 4]);
    }
    // The parked stamp itself is not in conflict with anything.
    assert_eq!(c.check(&parked), Ok(Readiness::Missing));
    assert_eq!(c.receive(&parked), Ok(Receipt::AlreadyHeld));
    assert_eq!(c, before);
}

#[test]
fn a_conflicting_stamp_is_refused_once_the_counter_is_covered() {
    let mut c = node(C);
    let parked = stamp(A, [2, 1, 0, 0]);
    assert_eq!(c.receive(&parked), Ok(Receipt::Held));
    assert_eq!(c.absorb(7, &Vv::from_counters([2, 0, 0, 0])), Ok(()));
    let before = c.clone();
    // Covered by the delivered clock, yet not the stamp that is parked.
    assert_eq!(c.check(&stamp(A, [2, 0, 0, 0])), Err(Refusal::Conflict));
    assert_eq!(c.receive(&stamp(A, [2, 0, 0, 0])), Err(Refusal::Conflict));
    assert_eq!(c.receive(&parked), Ok(Receipt::Duplicate));
    assert_eq!(c, before);
    // The parked stamp leaves by name, as obsolete; then nothing conflicts.
    assert_eq!(c.release(), Some(Release::Obsolete(parked)));
    assert_eq!(c.receive(&stamp(A, [2, 0, 0, 0])), Ok(Receipt::Duplicate));
}

#[test]
fn forgetting_the_parked_stamp_resolves_the_conflict() {
    let mut c = node(C);
    assert_eq!(c.receive(&stamp(A, [2, 1, 0, 0])), Ok(Receipt::Held));
    assert_eq!(c.receive(&stamp(A, [1, 0, 0, 0])), Ok(Receipt::Delivered));
    let other = stamp(A, [2, 0, 0, 0]);
    assert_eq!(c.receive(&other), Err(Refusal::Conflict));
    assert_eq!(c.forget(A, 2), Ok(Forgetting::Forgotten));
    assert_eq!(c.receive(&other), Ok(Receipt::Delivered));
    assert_eq!(c.held(), 0);
    assert_eq!(c.clock().counters(), &[2, 0, 0, 0]);
    // The forgotten stamp is now simply already delivered.
    assert_eq!(c.receive(&stamp(A, [2, 1, 0, 0])), Ok(Receipt::Duplicate));
}

#[test]
fn only_the_identical_stamp_takes_its_parked_copy_along() {
    let mut c = node(C);
    let late = stamp(A, [2, 1, 0, 0]);
    assert_eq!(c.receive(&late), Ok(Receipt::Held));
    assert_eq!(c.receive(&stamp(A, [1, 0, 0, 0])), Ok(Receipt::Delivered));
    assert_eq!(c.receive(&stamp(B, [0, 1, 0, 0])), Ok(Receipt::Delivered));
    // Ready and still parked: a different clock is refused, the same stamp
    // is delivered once and leaves nothing behind.
    assert_eq!(c.receive(&stamp(A, [2, 0, 0, 0])), Err(Refusal::Conflict));
    assert_eq!(c.held(), 1);
    assert_eq!(c.receive(&late), Ok(Receipt::Delivered));
    assert_eq!(c.held(), 0);
    assert_eq!(c.release(), None);
    assert_eq!(c.clock().counters(), &[2, 1, 0, 0]);
}
