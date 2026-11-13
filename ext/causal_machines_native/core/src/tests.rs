extern crate std;

mod conflict;
mod model;

use super::*;

type Vv = VersionVector<4>;
type Node = Causal<4, 2>;

const A: ReplicaId = 10;
const B: ReplicaId = 20;
const C: ReplicaId = 30;

fn members() -> Membership<4> {
    let Some(members) = Membership::from_slice(&[A, B, C]) else {
        panic!("three distinct replicas fit four slots");
    };
    members
}

fn node(local: ReplicaId) -> Node {
    let Some(node) = Causal::new(members(), local, 7) else {
        panic!("local replica is a member");
    };
    node
}

fn stamp(sender: ReplicaId, counters: [u64; 4]) -> Stamp<4> {
    Stamp::new(7, sender, Vv::from_counters(counters))
}

fn sent(node: &mut Node) -> Stamp<4> {
    let Ok(stamp) = node.stamp() else {
        panic!("counter has room");
    };
    stamp
}

fn gaps(node: &Node, stamp: &Stamp<4>, page: usize) -> std::vec::Vec<Gap> {
    let blank = Gap {
        replica: 0,
        from: 0,
        through: 0,
    };
    let mut all = std::vec::Vec::new();
    let mut cursor = Some(0);
    while let Some(at) = cursor {
        let mut out = std::vec![blank; page];
        let Ok(listed) = node.missing(stamp, at, &mut out) else {
            panic!("stamp is well formed");
        };
        all.extend_from_slice(&out[..listed.written]);
        assert!(
            listed
                .next
                .is_none_or(|next| next > at || listed.written == 0)
        );
        cursor = listed.next;
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
fn vectors_compare_as_a_partial_order() {
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
}

#[test]
fn merge_takes_the_larger_side_and_dominates_both() {
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
    let before = clock;
    assert_eq!(clock.increment(1), Err(Refusal::Exhausted));
    assert_eq!(clock.increment(4), Err(Refusal::UnknownMember));
    assert_eq!(clock, before);
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
    let before = c.clone();
    assert_eq!(c.receive(&stamp(A, [4, 0, 0, 0])), Err(Refusal::Full));
    assert_eq!(c, before);
    assert_eq!(c.receive(&stamp(A, [2, 1, 0, 0])), Err(Refusal::Conflict));
    assert_eq!(c, before);
    // Idempotent repeats are honoured and change nothing either.
    assert_eq!(c.receive(&stamp(A, [2, 0, 0, 0])), Ok(Receipt::AlreadyHeld));
    assert_eq!(c, before);
    assert_eq!(c.forget(99, 3), Err(Refusal::UnknownMember));
    assert_eq!(c.forget(B, 3), Ok(Forgetting::NotHeld));
    assert_eq!(c.forget(A, 9), Ok(Forgetting::NotHeld));
    assert_eq!(c, before);

    assert_eq!(c.forget(A, 3), Ok(Forgetting::Forgotten));
    assert_eq!(c.held(), 1);
    let forgotten = c.clone();
    assert_eq!(c.forget(A, 3), Ok(Forgetting::NotHeld));
    assert_eq!(c, forgotten);
    assert_eq!(c.receive(&stamp(A, [4, 0, 0, 0])), Ok(Receipt::Held));
    assert_eq!(c.receive(&stamp(A, [1, 0, 0, 0])), Ok(Receipt::Delivered));
    assert_eq!(c.release(), Some(Release::Deliver(stamp(A, [2, 0, 0, 0]))));
    assert_eq!(c.release(), None);
    assert_eq!(c.held(), 1);

    let Some(mut strict) = Causal::<4, 0>::new(members(), C, 7) else {
        panic!("local replica is a member");
    };
    assert_eq!(strict.receive(&stamp(A, [2, 0, 0, 0])), Err(Refusal::Full));
    assert_eq!(
        strict.receive(&stamp(A, [1, 0, 0, 0])),
        Ok(Receipt::Delivered)
    );
}

#[test]
fn delivering_directly_drops_the_parked_copy() {
    let mut c = node(C);
    let late = stamp(A, [2, 0, 0, 0]);
    assert_eq!(c.receive(&late), Ok(Receipt::Held));
    assert_eq!(c.receive(&stamp(A, [1, 0, 0, 0])), Ok(Receipt::Delivered));
    assert_eq!(c.receive(&late), Ok(Receipt::Delivered));
    assert_eq!(c.held(), 0);
    assert_eq!(c.release(), None);
}

#[test]
fn stale_future_unknown_and_malformed_stamps_are_refused_untouched() {
    let mut c = node(C);
    let before = c.clone();
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
        assert_eq!(c.receive(&bad), Err(why));
        assert_eq!(c.missing(&bad, 0, &mut []), Err(why));
    }
    assert_eq!(c.absorb(6, &good), Err(Refusal::StaleGeneration));
    assert_eq!(c.absorb(8, &good), Err(Refusal::FutureGeneration));
    let outside = Vv::from_counters([0, 0, 0, 1]);
    assert_eq!(c.absorb(7, &outside), Err(Refusal::Malformed));
    assert_eq!(c, before);
    assert_eq!(std::format!("{}", Refusal::Malformed), "malformed clock");
    assert_eq!(std::format!("{}", Refusal::Full), "hold table full");
    assert_ne!(
        std::format!("{}", Refusal::Conflict),
        std::format!("{}", Refusal::Full)
    );
}

#[test]
fn absorbing_a_clock_makes_covered_parked_stamps_obsolete() {
    let mut c = node(C);
    let third = stamp(A, [3, 0, 0, 0]);
    let second = stamp(A, [2, 0, 0, 0]);
    let fifth = stamp(A, [5, 1, 0, 0]);
    assert_eq!(c.receive(&third), Ok(Receipt::Held));
    assert_eq!(c.receive(&second), Ok(Receipt::Held));
    assert_eq!(c.absorb(7, &Vv::from_counters([4, 0, 0, 0])), Ok(()));
    assert_eq!(c.clock().counters(), &[4, 0, 0, 0]);
    assert_eq!(c.release(), Some(Release::Obsolete(second)));
    assert_eq!(c.release(), Some(Release::Obsolete(third)));
    assert_eq!(c.release(), None);
    assert_eq!(c.receive(&fifth), Ok(Receipt::Held));
    assert_eq!(c.absorb(7, &Vv::from_counters([0, 1, 0, 0])), Ok(()));
    assert_eq!(c.clock().counters(), &[4, 1, 0, 0]);
    assert_eq!(c.release(), Some(Release::Deliver(fifth)));
    assert_eq!(c.clock().counters(), &[5, 1, 0, 0]);
}

#[test]
fn an_older_stamp_after_a_newer_one_is_a_duplicate() {
    let mut c = node(C);
    assert_eq!(c.absorb(7, &Vv::from_counters([9, 0, 0, 0])), Ok(()));
    let before = c.clone();
    assert_eq!(c.receive(&stamp(A, [3, 0, 0, 0])), Ok(Receipt::Duplicate));
    assert_eq!(c.receive(&stamp(A, [9, 5, 0, 0])), Ok(Receipt::Duplicate));
    assert_eq!(c, before);
}

#[test]
fn missing_dependencies_page_through_a_bounded_buffer() {
    let c = node(C);
    let wanted = stamp(B, [3, 4, 2, 0]);
    let expected = [
        Gap {
            replica: A,
            from: 1,
            through: 3,
        },
        Gap {
            replica: B,
            from: 1,
            through: 3,
        },
        Gap {
            replica: C,
            from: 1,
            through: 2,
        },
    ];
    for page in 1..=4 {
        assert_eq!(gaps(&c, &wanted, page), expected);
    }
    let mut one = [expected[0]; 1];
    assert_eq!(
        c.missing(&wanted, 1, &mut one),
        Ok(Page {
            written: 1,
            next: Some(2)
        })
    );
    assert_eq!(one[0], expected[1]);
    assert_eq!(
        c.missing(&wanted, 0, &mut []),
        Ok(Page {
            written: 0,
            next: Some(0)
        })
    );
    assert_eq!(
        c.missing(&wanted, 9, &mut one),
        Ok(Page {
            written: 0,
            next: None
        })
    );
    assert_eq!(gaps(&c, &stamp(A, [1, 0, 0, 0]), 2), []);
}

#[test]
fn counters_at_the_u64_limit_neither_wrap_nor_reissue() {
    let top = Vv::from_counters([u64::MAX - 1, 0, 0, 0]);
    let Some(mut a) = Node::restore(members(), A, 7, top) else {
        panic!("clock stays inside the membership");
    };
    let last = sent(&mut a);
    assert_eq!(last.clock.get(0), Some(u64::MAX));
    let before = a.clone();
    assert_eq!(a.stamp(), Err(Refusal::Exhausted));
    assert_eq!(a, before);

    let mut c = node(C);
    assert_eq!(c.receive(&last), Ok(Receipt::Held));
    assert_eq!(
        gaps(&c, &last, 4),
        [Gap {
            replica: A,
            from: 1,
            through: u64::MAX - 1
        }]
    );
    assert_eq!(c.absorb(7, &top), Ok(()));
    assert_eq!(c.release(), Some(Release::Deliver(last)));
    assert_eq!(c.clock().get(0), Some(u64::MAX));
    assert_eq!(c.receive(&last), Ok(Receipt::Duplicate));
}

#[test]
fn a_default_buffer_is_filled_from_the_front() {
    let c = node(C);
    let mut out = [Gap::default(); 4];
    assert_eq!(out[0].from, 0);
    let page = c.missing(&stamp(B, [1, 2, 0, 0]), 0, &mut out);
    assert_eq!(page.map(|p| (p.written, p.next)), Ok((2, None)));
    let listed = [(A, 1, 1), (B, 1, 1)].map(|(replica, from, through)| Gap {
        replica,
        from,
        through,
    });
    assert_eq!(out[..2], listed);
    assert_eq!(out[2..], [Gap::default(); 2]);
}
