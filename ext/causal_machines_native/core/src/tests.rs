extern crate std;

mod conflict;
mod model;

use super::*;
use matryona_machines::testing::{refuses, unchanged};

type Vv = VersionVector<4>;
type Node = Causal<4, 2>;

const A: ReplicaId = 10;
const B: ReplicaId = 20;
const C: ReplicaId = 30;

fn members() -> Membership<4> {
    Membership::from_slice(&[A, B, C]).expect("three distinct replicas fit four slots")
}

fn node(local: ReplicaId) -> Node {
    Causal::new(members(), local, 7).expect("local replica is a member")
}

fn stamp(sender: ReplicaId, counters: [u64; 4]) -> Stamp<4> {
    Stamp::new(7, sender, Vv::from_counters(counters))
}

fn sent(node: &mut Node) -> Stamp<4> {
    node.stamp().expect("counter has room")
}

fn gap(replica: ReplicaId, from: u64, through: u64) -> Gap {
    Gap::new(replica, from, through).expect("counters run forward from 1")
}

fn gaps(node: &Node, stamp: &Stamp<4>, page: usize) -> std::vec::Vec<Gap> {
    let mut all = std::vec::Vec::new();
    let mut cursor = Some(0);
    while let Some(at) = cursor {
        let mut out = std::vec![Gap::default(); page];
        let listed = node
            .missing(stamp, at, &mut out)
            .expect("stamp is well formed");
        all.extend_from_slice(&out[..listed.written()]);
        assert!(
            listed
                .next()
                .is_none_or(|next| next > at || listed.written() == 0)
        );
        cursor = listed.next();
    }
    all
}

#[test]
fn membership_binds_distinct_replicas_to_slots() {
    let members = members();
    assert_eq!(members.len(), 3);
    assert!(!members.is_empty());
    assert_eq!(members.ids(), &[A, B, C]);
    assert_eq!(members.slot(B), Some(1));
    assert_eq!(members.slot(99), None);
    assert_eq!(members.replica(2), Some(C));
    assert_eq!(members.replica(3), None);
    assert!(members.admits(&Vv::from_counters([1, 2, 3, 0])));
    assert!(!members.admits(&Vv::from_counters([0, 0, 0, 1])));
    assert_eq!(Membership::<4>::from_slice(&[A, B, A]), None);
    assert_eq!(Membership::<2>::from_slice(&[A, B, C]), None);
    assert!(Membership::<0>::from_slice(&[]).is_some_and(|empty| empty.is_empty()));
}

#[test]
fn vectors_compare_and_merge_as_a_partial_order() {
    let base = Vv::from_counters([1, 2, 0, 0]);
    let later = Vv::from_counters([1, 3, 0, 0]);
    let fork = Vv::from_counters([2, 1, 0, 0]);
    assert_eq!(base.compare(&base), Causality::Equal);
    assert_eq!(base.compare(&later), Causality::Before);
    assert_eq!(later.compare(&base), Causality::After);
    assert_eq!(base.compare(&fork), Causality::Concurrent);
    assert!(later.dominates(&base) && base.dominates(&base));
    assert!(!base.dominates(&later) && !base.dominates(&fork));
    assert!(base.concurrent(&fork) && !base.concurrent(&later));
    assert_eq!(Vv::default(), Vv::new());
    assert_eq!(base.get(1), Some(2));
    assert_eq!(base.get(4), None);
    assert_eq!(base.counters(), &[1, 2, 0, 0]);
    // Merging takes the larger side and dominates both.
    let left = Vv::from_counters([5, 0, u64::MAX, 1]);
    let right = Vv::from_counters([2, 9, 3, 1]);
    let joined = left.merged(&right);
    assert_eq!(joined.counters(), &[5, 9, u64::MAX, 1]);
    assert_eq!(joined, right.merged(&left));
    assert_eq!(joined.merged(&joined), joined);
    assert!(joined.dominates(&left) && joined.dominates(&right));
    let mut in_place = left;
    in_place.merge(&right);
    assert_eq!(in_place, joined);
}

#[test]
fn increment_refuses_unknown_slots_and_exhausted_counters() {
    let mut clock = Vv::from_counters([0, u64::MAX - 1, 0, 0]);
    assert_eq!(clock.increment(0), Ok(1));
    assert_eq!(clock.increment(1), Ok(u64::MAX));
    refuses(&mut clock, Refusal::Exhausted, |c| c.increment(1));
    refuses(&mut clock, Refusal::UnknownMember, |c| c.increment(4));
}

#[test]
fn messages_deliver_in_causal_order_whatever_order_they_arrive() {
    let mut a = node(A);
    let mut b = node(B);
    let mut c = node(C);
    let first = sent(&mut a);
    assert_eq!(b.receive(&first), Ok(Receipt::Delivered));
    let reply = sent(&mut b);
    assert_eq!(reply.clock.counters(), &[1, 1, 0, 0]);

    assert_eq!(c.check(&reply), Ok(Readiness::Missing));
    assert_eq!(c.receive(&reply), Ok(Receipt::Held));
    assert_eq!(c.receive(&reply), Ok(Receipt::AlreadyHeld));
    assert_eq!(c.held(), 1);
    assert_eq!(c.release(), None);
    assert_eq!(c.check(&first), Ok(Readiness::Deliverable));
    assert_eq!(c.receive(&first), Ok(Receipt::Delivered));
    assert_eq!(c.release(), Some(Release::Deliver(reply)));
    assert_eq!(c.release(), None);
    assert_eq!(c.held(), 0);
    assert_eq!(c.clock().counters(), &[1, 1, 0, 0]);
    assert_eq!(c.receive(&first), Ok(Receipt::Duplicate));
    assert_eq!(c.check(&reply), Ok(Readiness::Duplicate));
    assert_eq!(a.receive(&first), Ok(Receipt::Duplicate));
}

#[test]
fn accessors_report_the_configuration() {
    let node = node(B);
    assert_eq!(node.membership(), &members());
    assert_eq!(node.local(), B);
    assert_eq!(node.generation(), 7);
    assert_eq!(node.capacity(), 2);
    assert_eq!(node.held(), 0);
    assert_eq!(node.clock(), &Vv::new());
    assert_eq!(Node::new(members(), 99, 7), None);
    let outside = Vv::from_counters([0, 0, 0, 1]);
    assert_eq!(Node::restore(members(), A, 7, outside), None);
}

#[test]
fn a_full_hold_table_refuses_without_mutating() {
    let mut c = node(C);
    assert_eq!(c.receive(&stamp(A, [2, 0, 0, 0])), Ok(Receipt::Held));
    assert_eq!(c.receive(&stamp(A, [3, 0, 0, 0])), Ok(Receipt::Held));
    refuses(&mut c, Refusal::Full, |c| {
        c.receive(&stamp(A, [4, 0, 0, 0]))
    });
    refuses(&mut c, Refusal::Conflict, |c| {
        c.receive(&stamp(A, [2, 1, 0, 0]))
    });
    // Idempotent repeats are honoured and change nothing either.
    assert_eq!(
        unchanged(&mut c, |c| c.receive(&stamp(A, [2, 0, 0, 0]))),
        Ok(Receipt::AlreadyHeld)
    );
    refuses(&mut c, Refusal::UnknownMember, |c| c.forget(99, 3));
    assert_eq!(
        unchanged(&mut c, |c| c.forget(B, 3)),
        Ok(Forgetting::NotHeld)
    );
    assert_eq!(
        unchanged(&mut c, |c| c.forget(A, 9)),
        Ok(Forgetting::NotHeld)
    );

    assert_eq!(c.forget(A, 3), Ok(Forgetting::Forgotten));
    assert_eq!(c.held(), 1);
    assert_eq!(
        unchanged(&mut c, |c| c.forget(A, 3)),
        Ok(Forgetting::NotHeld)
    );
    assert_eq!(c.receive(&stamp(A, [4, 0, 0, 0])), Ok(Receipt::Held));
    assert_eq!(c.receive(&stamp(A, [1, 0, 0, 0])), Ok(Receipt::Delivered));
    assert_eq!(c.release(), Some(Release::Deliver(stamp(A, [2, 0, 0, 0]))));
    assert_eq!(c.release(), None);
    assert_eq!(c.held(), 1);

    let mut strict = Causal::<4, 0>::new(members(), C, 7).expect("local replica is a member");
    assert_eq!(strict.receive(&stamp(A, [2, 0, 0, 0])), Err(Refusal::Full));
    assert_eq!(
        strict.receive(&stamp(A, [1, 0, 0, 0])),
        Ok(Receipt::Delivered)
    );
}

#[test]
fn stale_future_unknown_and_malformed_stamps_are_refused_untouched() {
    let mut c = node(C);
    let good = Vv::from_counters([1, 0, 0, 0]);
    let cases = [
        (Stamp::new(6, A, good), Refusal::StaleGeneration),
        (Stamp::new(8, A, good), Refusal::FutureGeneration),
        (Stamp::new(7, 99, good), Refusal::UnknownMember),
        (stamp(A, [0, 1, 0, 0]), Refusal::Malformed),
        (stamp(A, [1, 0, 0, 1]), Refusal::Malformed),
    ];
    for (bad, why) in cases {
        assert_eq!(c.check(&bad), Err(why));
        refuses(&mut c, why, |c| c.receive(&bad));
        assert_eq!(c.missing(&bad, 0, &mut []), Err(why));
    }
    refuses(&mut c, Refusal::StaleGeneration, |c| c.absorb(6, &good));
    refuses(&mut c, Refusal::FutureGeneration, |c| c.absorb(8, &good));
    let outside = Vv::from_counters([0, 0, 0, 1]);
    refuses(&mut c, Refusal::Malformed, |c| c.absorb(7, &outside));
    assert_eq!(std::format!("{}", Refusal::Malformed), "malformed clock");
    assert_eq!(std::format!("{}", Refusal::Full), "hold table full");
}

#[test]
fn absorbing_a_clock_makes_covered_parked_stamps_obsolete() {
    let mut c = node(C);
    let third = stamp(A, [3, 0, 0, 0]);
    let second = stamp(A, [2, 0, 0, 0]);
    let fifth = stamp(A, [5, 1, 0, 0]);
    assert_eq!(c.receive(&third), Ok(Receipt::Held));
    assert_eq!(c.receive(&second), Ok(Receipt::Held));
    assert_eq!(
        c.absorb(7, &Vv::from_counters([4, 0, 0, 0])),
        Ok(Absorbing::Absorbed)
    );
    assert_eq!(c.clock().counters(), &[4, 0, 0, 0]);
    assert_eq!(c.release(), Some(Release::Obsolete(second)));
    assert_eq!(c.release(), Some(Release::Obsolete(third)));
    assert_eq!(c.release(), None);
    assert_eq!(c.receive(&fifth), Ok(Receipt::Held));
    assert_eq!(
        c.absorb(7, &Vv::from_counters([0, 1, 0, 0])),
        Ok(Absorbing::Absorbed)
    );
    assert_eq!(c.clock().counters(), &[4, 1, 0, 0]);
    assert_eq!(c.release(), Some(Release::Deliver(fifth)));
    assert_eq!(c.clock().counters(), &[5, 1, 0, 0]);
}

#[test]
fn missing_dependencies_page_through_a_bounded_buffer() {
    let c = node(C);
    let wanted = stamp(B, [3, 4, 2, 0]);
    let expected = [gap(A, 1, 3), gap(B, 1, 3), gap(C, 1, 2)];
    for page in 1..=4 {
        assert_eq!(gaps(&c, &wanted, page), expected);
    }
    // A cursor resumes at its slot; the buffer fills from the front and the
    // rest is left untouched.
    let mut out = [Gap::default(); 3];
    let page = c.missing(&wanted, 1, &mut out[..1]);
    assert_eq!(page.map(|p| (p.written(), p.next())), Ok((1, Some(2))));
    assert_eq!(out, [expected[1], Gap::default(), Gap::default()]);
    let page = c.missing(&wanted, 0, &mut []);
    assert_eq!(page.map(|p| (p.written(), p.next())), Ok((0, Some(0))));
    assert_eq!(gaps(&c, &stamp(A, [1, 0, 0, 0]), 2), []);
}

#[test]
fn a_page_is_only_what_missing_could_answer() {
    assert_eq!(
        Page::new::<4>(1, Some(2)).map(|p| (p.written(), p.next())),
        Some((1, Some(2)))
    );
    assert!(Page::new::<4>(4, None).is_some());
    assert!(Page::new::<4>(0, Some(0)).is_some());
    assert!(Page::new::<4>(3, Some(3)).is_some());
    assert_eq!(Page::new::<4>(5, None), None); // more gaps than slots
    assert_eq!(Page::new::<4>(0, Some(4)), None); // a cursor past the slots
    assert_eq!(Page::new::<4>(3, Some(2)), None); // more gaps than slots before the cursor
    // Every page `missing` answers is one `Page::new` builds.
    let c = node(C);
    let wanted = stamp(B, [3, 4, 2, 0]);
    for limit in 0..=4 {
        for cursor in 0..4 {
            let mut out = [Gap::default(); 4];
            if let Ok(page) = c.missing(&wanted, cursor, &mut out[..limit]) {
                assert_eq!(Page::new::<4>(page.written(), page.next()), Some(page));
            }
        }
    }
}

#[test]
fn a_cursor_the_tracker_never_issued_is_refused() {
    let c = node(C);
    let mut out = [Gap::default(); 4];
    // Past the three member slots, whatever the stamp's readiness.
    for wanted in [stamp(B, [3, 4, 2, 0]), stamp(A, [1, 0, 0, 0])] {
        for cursor in [3, 4, usize::MAX] {
            assert_eq!(
                c.missing(&wanted, cursor, &mut out),
                Err(Refusal::UnknownMember)
            );
        }
    }
    assert_eq!(out, [Gap::default(); 4]);
    // The stamp is judged first.
    let stale = Stamp::new(6, A, Vv::from_counters([1, 0, 0, 0]));
    assert_eq!(
        c.missing(&stale, 9, &mut out),
        Err(Refusal::StaleGeneration)
    );
}

#[test]
fn counters_at_the_u64_limit_neither_wrap_nor_reissue() {
    let top = Vv::from_counters([u64::MAX - 1, 0, 0, 0]);
    let mut a = Node::restore(members(), A, 7, top).expect("clock stays inside the membership");
    let last = sent(&mut a);
    assert_eq!(last.clock.get(0), Some(u64::MAX));
    refuses(&mut a, Refusal::Exhausted, Node::stamp);

    let mut c = node(C);
    assert_eq!(c.receive(&last), Ok(Receipt::Held));
    assert_eq!(gaps(&c, &last, 4), [gap(A, 1, u64::MAX - 1)]);
    assert_eq!(c.absorb(7, &top), Ok(Absorbing::Absorbed));
    assert_eq!(c.release(), Some(Release::Deliver(last)));
    assert_eq!(c.clock().get(0), Some(u64::MAX));
    assert_eq!(c.receive(&last), Ok(Receipt::Duplicate));
}

#[test]
fn covered_clocks_and_stamps_are_honoured_repeats() {
    let mut c = node(C);
    assert_eq!(
        c.absorb(7, &Vv::from_counters([4, 1, 0, 0])),
        Ok(Absorbing::Absorbed)
    );
    for covered in [[4, 1, 0, 0], [2, 0, 0, 0], [0, 0, 0, 0]] {
        assert_eq!(
            unchanged(&mut c, |c| c.absorb(7, &Vv::from_counters(covered))),
            Ok(Absorbing::AlreadyCovered)
        );
    }
    // An older stamp after a newer one is a duplicate, whatever it depends on.
    for older in [[3, 0, 0, 0], [4, 5, 0, 0]] {
        assert_eq!(
            unchanged(&mut c, |c| c.receive(&stamp(A, older))),
            Ok(Receipt::Duplicate)
        );
    }
    assert_eq!(
        c.absorb(7, &Vv::from_counters([3, 2, 0, 0])),
        Ok(Absorbing::Absorbed)
    );
    assert_eq!(c.clock().counters(), &[4, 2, 0, 0]);
}

#[test]
fn gaps_are_built_only_as_missing_emits_them() {
    let run = gap(A, 1, 3);
    assert_eq!((run.replica(), run.from(), run.through()), (A, 1, 3));
    assert_eq!(gap(A, u64::MAX, u64::MAX).from(), u64::MAX);
    assert_eq!(Gap::new(A, 0, 3), None);
    assert_eq!(Gap::new(A, 4, 3), None);
}
