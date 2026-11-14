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
hands back parked stamps one at a time as their dependencies arrive (`None`
means nothing is ready), `forget` drops a parked stamp and says whether there
was one (`Forgetting::Forgotten` or `Forgetting::NotHeld`), and
`missing` lists the undelivered runs per replica into a caller buffer, resumable
by cursor. A cursor is a membership slot: 0 or a `Page::next` the tracker
handed out. One past the last slot was never issued and is refused with
`UnknownMember`, after the stamp itself is judged. It writes the first
`Page::written` entries of `out` and leaves the rest untouched (a refusal
writes none); build the buffer as `[Gap::default(); K]`. The default is a
placeholder the core never emits, because counters start at 1. A `Page`'s
fields are private: `Page::new::<N>(written, next)` rebuilds one and answers
`None` for a page `missing` could not have answered (more gaps than slots, a
cursor past the slots, or more gaps than slots before the cursor).

`Refusal` is the only refusal type, and no public enum is `#[non_exhaustive]`:
a new variant is a breaking change every caller matches on. A refused call was not honoured and leaves
the whole value untouched; `Causal` is `PartialEq`, so comparing against a
clone proves it. A stamp from another membership generation is refused with
`StaleGeneration` or `FutureGeneration`, an unknown sender with
`UnknownMember`, a clock counting outside the membership (or a zero sender
entry) with `Malformed`, and a counter at `u64::MAX` with `Exhausted`. A stamp
that must wait is refused with `Full` when no hold slot is free.

A stamp is refused with `Conflict` when a different stamp with the same sender
and counter is parked, on every path: whether the incoming stamp would have
been delivered, parked, or called a duplicate. Two stamps numbered alike with
different clocks cannot both be the sender's message, and the tracker cannot
tell which is, so it never drops or replaces the parked one for the other.
`check` and `missing` refuse the same way, so `check` answers what `receive`
would do. Parked stamps leave the hold table three ways only: handed back by
`release` (`Deliver` or `Obsolete`), dropped by name with `forget`, which is
how a caller resolves a conflict in favour of the refused stamp, or delivered
by `receive` when the identical stamp arrives again and is now ready. `absorb`
never touches the hold table.

`absorb` answers `Absorbing::Absorbed` when the clock advanced the delivered
clock and `Absorbing::AlreadyCovered` when the delivered clock already
dominated it: an idempotent repeat, provable from the delivered clock alone,
that changes nothing.

| Operation | Honoured | Refusals |
| --- | --- | --- |
| `stamp() -> Result<Stamp, Refusal>` | the stamp | `Exhausted` |
| `check(&stamp) -> Result<Readiness, Refusal>` | `Deliverable`, `Duplicate`, `Missing` | `StaleGeneration`, `FutureGeneration`, `UnknownMember`, `Malformed`, `Conflict` |
| `receive(&stamp) -> Result<Receipt, Refusal>` | `Delivered`, `Held`, `AlreadyHeld`, `Duplicate` | as `check`, plus `Full` |
| `release() -> Option<Release>` | a release, or `None` when nothing is ready | none |
| `forget(sender, counter) -> Result<Forgetting, Refusal>` | `Forgotten`, `NotHeld` | `UnknownMember` |
| `missing(&stamp, cursor, out) -> Result<Page, Refusal>` | the page | as `check`, then `UnknownMember` for a cursor past the last membership slot |
| `absorb(generation, &clock) -> Result<Absorbing, Refusal>` | `Absorbed`, `AlreadyCovered` | `StaleGeneration`, `FutureGeneration`, `Malformed` |
| `VersionVector::increment(slot) -> Result<u64, Refusal>` | the new counter | `UnknownMember`, `Exhausted` |

`Receipt::AlreadyHeld`, `Receipt::Duplicate`, `Forgetting::NotHeld` and
`Absorbing::AlreadyCovered` are idempotent repeats: what the caller asked for
already holds, so the call is honoured and changes nothing. In Ruby every
honoured outcome is the snake-case symbol of its variant.

No operation takes or records a clock: the core has no notion of time, so no
answer depends on `now`. It issues no ids either: replicas, generations and
counters are the caller's.

The values validate where they are built. `Membership::from_slice` refuses
duplicates and more than `N` ids, `Gap::new` refuses a run starting at 0 or
ending before it starts (its fields are private, read through `replica()`,
`from()` and `through()`, so no `Gap` skips that check), and `Stamp::new` and `VersionVector::from_counters`
accept every input; the receiving tracker judges a stamp. In release builds no
operation panics: an arm the invariants rule out (a membership slot past `N`)
is a `debug_assert!` that answers the most conservative result with nothing
changed.

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
let mut gaps = [Gap::default(); 3];
let page = carol.missing(&answer, 0, &mut gaps).expect("well formed");
assert_eq!((page.written(), page.next()), (1, None));
assert_eq!(Some(gaps[0]), Gap::new(1, 1, 1));

assert_eq!(carol.receive(&question), Ok(Receipt::Delivered));
assert_eq!(carol.release(), Some(Release::Deliver(answer)));
assert_eq!(carol.release(), None);
```
