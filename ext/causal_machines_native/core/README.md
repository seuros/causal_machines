# causal-machines

> **AMS Fidge**, NCC-4231, *Fidge*-class causality cutter.

Engine-room flight manual: decide whether a dispatch may be opened yet, from
the vector it was stamped with.

Allocation-free `no_std` version vectors over a fixed membership, plus the
causal-delivery readiness check built on them. `VersionVector<N>` compares as a
partial order (`Equal`, `Before`, `After`, `Concurrent`), answers dominance and
concurrency, and merges by pointwise maximum. `Membership<N>` binds up to `N`
distinct replica ids to slots once; a different set of replicas is a different
membership generation.

`Causal<N, P>` is one replica's delivery state: the delivered clock and `P` hold
slots for the stamps of messages that arrived ahead of their dependencies. A
stamp is `Deliverable` when it is the sender's next message and everything the
sender had delivered is delivered here; `Duplicate` when the delivered clock
already covers it; otherwise `Missing`. `receive` delivers or parks, `release`
hands back parked stamps one at a time as their dependencies arrive, and
`missing` lists the undelivered runs per replica into a caller buffer, resumable
by cursor.

Refusals are explicit and leave state untouched: a full hold table answers
`Full`, a stamp from another membership generation answers `StaleGeneration` or
`FutureGeneration`, an unknown sender `UnknownMember`, a clock counting outside
the membership `Malformed`, and a counter at `u64::MAX` `Exhausted`.

This is a policy core. It stores stamps, never payloads. It has no transport, no
retransmission, no persistence, no clock and no membership change protocol. The
caller keys its own payload buffer by `(sender, counter)`, asks peers for the
gaps `missing` reports, and persists the delivered clock if a restart must not
reissue counters (`Causal::restore`).

```rust
use causal_machines::{Causal, Gap, Membership, Receipt, Release};

let members = Membership::<3>::from_slice(&[1, 2, 3]).expect("distinct ids");
let mut alice = Causal::<3, 4>::new(members, 1, 0).expect("member");
let mut bob = Causal::<3, 4>::new(members, 2, 0).expect("member");
let mut carol = Causal::<3, 4>::new(members, 3, 0).expect("member");

let question = alice.stamp().expect("counter has room");
assert_eq!(bob.receive(&question), Ok(Receipt::Delivered));
let answer = bob.stamp().expect("counter has room");

// The answer overtakes the question on the way to Carol.
assert_eq!(carol.receive(&answer), Ok(Receipt::Held));
let mut gaps = [Gap { replica: 0, from: 0, through: 0 }; 3];
let page = carol.missing(&answer, 0, &mut gaps).expect("well formed");
assert_eq!((page.written, page.next), (1, None));
assert_eq!(gaps[0], Gap { replica: 1, from: 1, through: 1 });

assert_eq!(carol.receive(&question), Ok(Receipt::Delivered));
assert_eq!(carol.release(), Some(Release::Deliver(answer)));
assert_eq!(carol.release(), None);
```
