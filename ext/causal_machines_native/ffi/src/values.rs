//! Validating hooks for the `Stamp`, `Gap`, `Page`, `Deliver` and `Obsolete`
//! values, and the conversions the tracker uses.
use causal_machines::{Gap, Page, Stamp};
use magnus::{Error, RArray, Ruby, Value, prelude::*};

use crate::classes::{classes, exact, field};
use crate::vectors::{Number, clock, clock_value, entry};
use crate::{MEMBERS, range_error};

/// A `CausalMachines::Stamp` of `stamp`, built through `new`.
pub fn stamp_value(ruby: &Ruby, stamp: &Stamp<MEMBERS>) -> Result<Value, Error> {
    let clock = clock_value(ruby, &stamp.clock)?;
    let class = ruby.get_inner(classes(ruby)?.stamp);
    class.funcall("new", (stamp.generation, stamp.sender, clock))
}

/// A `CausalMachines::Gap` of `gap`, built through `new`.
pub fn gap_value(ruby: &Ruby, gap: &Gap) -> Result<Value, Error> {
    let class = ruby.get_inner(classes(ruby)?.gap);
    class.funcall("new", (gap.replica(), gap.from(), gap.through()))
}

/// What `Stamp.new` stores: `Stamp::new` takes any generation, sender and
/// vector, so only types and ranges are checked.
pub fn checked_stamp(
    ruby: &Ruby,
    generation: Number,
    sender: Number,
    clock: Value,
) -> Result<(u64, u64, Value), Error> {
    self::clock(ruby, clock)?;
    Ok((generation.0, sender.0, clock))
}

/// The core stamp of a `CausalMachines::Stamp`.
pub fn stamp(ruby: &Ruby, value: Value) -> Result<Stamp<MEMBERS>, Error> {
    let held = exact(
        ruby,
        value,
        classes(ruby)?.stamp,
        "expected a CausalMachines::Stamp",
    )?;
    let generation: Number = field(held, 0)?;
    let sender: Number = field(held, 1)?;
    let seen = clock(ruby, field(held, 2)?)?;
    Ok(Stamp::new(generation.0, sender.0, seen))
}

/// What `Deliver.new` and `Obsolete.new` store: a `CausalMachines::Stamp`.
pub fn checked_release(ruby: &Ruby, held: Value) -> Result<Value, Error> {
    stamp(ruby, held)?;
    Ok(held)
}

/// The core gap of a `CausalMachines::Gap`, rebuilt through `Gap::new`.
fn gap(ruby: &Ruby, value: Value) -> Result<Gap, Error> {
    let held = exact(
        ruby,
        value,
        classes(ruby)?.gap,
        "expected a CausalMachines::Gap",
    )?;
    checked_gap(ruby, field(held, 0)?, field(held, 1)?, field(held, 2)?)
}

fn checked_gap(ruby: &Ruby, replica: Number, from: Number, through: Number) -> Result<Gap, Error> {
    Gap::new(replica.0, from.0, through.0).ok_or_else(|| {
        range_error(
            ruby,
            "a gap runs from a counter of at least 1 through one no smaller",
        )
    })
}

/// What `Gap.new` stores: the parts `Gap::new` accepts.
pub fn checked_gap_fields(
    ruby: &Ruby,
    replica: Number,
    from: Number,
    through: Number,
) -> Result<(u64, u64, u64), Error> {
    let gap = checked_gap(ruby, replica, from, through)?;
    Ok((gap.replica(), gap.from(), gap.through()))
}

/// What `Page.new` stores: the gaps (each a `CausalMachines::Gap`, frozen
/// into a copy) and the cursor of a page `Page::new` accepts.
pub fn checked_page(
    ruby: &Ruby,
    gaps: Value,
    next: Option<Number>,
) -> Result<(RArray, Option<u64>), Error> {
    let Some(gaps) = RArray::from_value(gaps) else {
        return Err(Error::new(
            ruby.exception_type_error(),
            "expected an Array of CausalMachines::Gap",
        ));
    };
    let held = ruby.ary_new_capa(gaps.len());
    for index in 0..gaps.len() {
        let each: Value = entry(gaps, index)?;
        gap(ruby, each)?;
        held.push(each)?;
    }
    Page::new::<MEMBERS>(held.len(), next.map(Number::index)).ok_or_else(|| {
        range_error(
            ruby,
            "no listing answers this page: one gap per slot before the cursor, \
             and the cursor names a membership slot",
        )
    })?;
    held.freeze();
    Ok((held, next.map(|cursor| cursor.0)))
}
