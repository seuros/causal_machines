//! Validating hooks for the `Stamp`, `Gap`, `Page`, `Deliver` and `Obsolete`
//! values, and the conversions the tracker uses.
use causal_machines::{Gap, Page, Stamp};
use magnus::{Error, RArray, Ruby, Value, prelude::*};
use matryona_machines::ruby::{exact_array, exact_fields, integer, optional};

use crate::classes::classes;
use crate::vectors::{clock, clock_value, entry};
use crate::{MEMBERS, range_error};

/// A `CausalMachines::Stamp` of `stamp`, built through `new`.
pub fn stamp_value(ruby: &Ruby, stamp: &Stamp<MEMBERS>) -> Result<Value, Error> {
    let clock = clock_value(ruby, &stamp.clock)?;
    let class = classes(ruby)?.stamp.get(ruby);
    class.funcall("new", (stamp.generation, stamp.sender, clock))
}

/// A `CausalMachines::Gap` of `gap`, built through `new`.
pub fn gap_value(ruby: &Ruby, gap: &Gap) -> Result<Value, Error> {
    let class = classes(ruby)?.gap.get(ruby);
    class.funcall("new", (gap.replica(), gap.from(), gap.through()))
}

/// What `Stamp.new` stores: `Stamp::new` takes any generation, sender and
/// vector, so only types and ranges are checked.
pub fn checked_stamp(
    ruby: &Ruby,
    generation: Value,
    sender: Value,
    clock: Value,
) -> Result<(u64, u64, Value), Error> {
    let (generation, sender) = (
        integer(generation, "generation")?,
        integer(sender, "sender")?,
    );
    self::clock(ruby, clock)?;
    Ok((generation, sender, clock))
}

/// The core stamp of a `CausalMachines::Stamp`.
pub fn stamp(ruby: &Ruby, value: Value) -> Result<Stamp<MEMBERS>, Error> {
    let message = "expected a CausalMachines::Stamp";
    let [generation, sender, seen] =
        exact_fields(ruby, value, classes(ruby)?.stamp.get(ruby), message)?;
    let generation = integer(generation, "generation")?;
    let sender = integer(sender, "sender")?;
    let seen = clock(ruby, seen)?;
    Ok(Stamp::new(generation, sender, seen))
}

/// What `Deliver.new` and `Obsolete.new` store: a `CausalMachines::Stamp`.
pub fn checked_release(ruby: &Ruby, held: Value) -> Result<Value, Error> {
    stamp(ruby, held)?;
    Ok(held)
}

/// The core gap of a `CausalMachines::Gap`, rebuilt through `Gap::new`.
fn gap(ruby: &Ruby, value: Value) -> Result<Gap, Error> {
    let message = "expected a CausalMachines::Gap";
    let [replica, from, through] =
        exact_fields(ruby, value, classes(ruby)?.gap.get(ruby), message)?;
    checked_gap(ruby, replica, from, through)
}

fn checked_gap(ruby: &Ruby, replica: Value, from: Value, through: Value) -> Result<Gap, Error> {
    let replica = integer(replica, "replica")?;
    let (from, through) = (integer(from, "from")?, integer(through, "through")?);
    Gap::new(replica, from, through).ok_or_else(|| {
        range_error(
            ruby,
            "a gap runs from a counter of at least 1 through one no smaller",
        )
    })
}

/// What `Gap.new` stores: the parts `Gap::new` accepts.
pub fn checked_gap_fields(
    ruby: &Ruby,
    replica: Value,
    from: Value,
    through: Value,
) -> Result<(u64, u64, u64), Error> {
    let gap = checked_gap(ruby, replica, from, through)?;
    Ok((gap.replica(), gap.from(), gap.through()))
}

/// What `Page.new` stores: the gaps (each a `CausalMachines::Gap`, frozen
/// into a copy) and the cursor of a page `Page::new` accepts.
pub fn checked_page(ruby: &Ruby, gaps: Value, next: Value) -> Result<(RArray, Option<u64>), Error> {
    let next: Option<u64> = optional(next, "next")?;
    let gaps = exact_array(ruby, gaps, "expected an Array of CausalMachines::Gap")?;
    let held = ruby.ary_new_capa(gaps.len());
    for index in 0..gaps.len() {
        let each = entry(gaps, index)?;
        gap(ruby, each)?;
        held.push(each)?;
    }
    let cursor = next.map(|cursor| usize::try_from(cursor).unwrap_or(usize::MAX));
    Page::new::<MEMBERS>(held.len(), cursor).ok_or_else(|| {
        range_error(
            ruby,
            "no listing answers this page: one gap per slot before the cursor, \
             and the cursor names a membership slot",
        )
    })?;
    held.freeze();
    Ok((held, next))
}
