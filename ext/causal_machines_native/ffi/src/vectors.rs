//! The `VersionVector` and `Membership` values: their validating hooks, the
//! conversions to and from the core types, and their methods.
use causal_machines::{Causality, Membership, VersionVector};
use magnus::{Error, Integer, RArray, Ruby, TryConvert, Value, prelude::*};

use crate::classes::{classes, exact, field};
use crate::{MEMBERS, range_error, refusal};

pub type Clock = VersionVector<MEMBERS>;
pub type Members = Membership<MEMBERS>;

/// An unsigned 64-bit Integer; anything else is a `TypeError`.
#[derive(Clone, Copy)]
pub struct Number(pub u64);

impl TryConvert for Number {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(Error::new(
                Ruby::get_with(value).exception_type_error(),
                "expected an Integer (unsigned 64-bit)",
            ));
        }
        u64::try_convert(value).map(Self)
    }
}

impl Number {
    /// As an index; past the address space is past every slot.
    pub fn index(self) -> usize {
        usize::try_from(self.0).unwrap_or(usize::MAX)
    }
}

/// The Integers of a Ruby Array of at most [`MEMBERS`] entries.
fn numbers(ruby: &Ruby, value: Value) -> Result<([u64; MEMBERS], usize), Error> {
    let Some(array) = RArray::from_value(value) else {
        return Err(Error::new(
            ruby.exception_type_error(),
            "expected an Array of Integers",
        ));
    };
    let len = array.len();
    if len > MEMBERS {
        return Err(range_error(ruby, format!("at most {MEMBERS} entries fit")));
    }
    let mut out = [0; MEMBERS];
    for (index, slot) in out.iter_mut().enumerate().take(len) {
        *slot = entry::<Number>(array, index)?.0;
    }
    Ok((out, len))
}

/// Entry `index` of `array`; past `isize::MAX` reads as past the end.
pub fn entry<T: TryConvert>(array: RArray, index: usize) -> Result<T, Error> {
    array.entry(isize::try_from(index).unwrap_or(isize::MAX))
}

/// A frozen Ruby Array of `values`.
fn frozen(ruby: &Ruby, values: &[u64]) -> Result<RArray, Error> {
    let out = ruby.ary_new_capa(values.len());
    for value in values {
        out.push(*value)?;
    }
    out.freeze();
    Ok(out)
}

/// What `VersionVector.new` stores: every one of the core's `MEMBERS`
/// counters, the ones not given as zero (`from_counters`).
pub fn checked_counters(ruby: &Ruby, counters: Value) -> Result<RArray, Error> {
    let (counters, _) = numbers(ruby, counters)?;
    frozen(ruby, Clock::from_counters(counters).counters())
}

/// The core vector of a `CausalMachines::VersionVector`.
pub fn clock(ruby: &Ruby, value: Value) -> Result<Clock, Error> {
    let message = "expected a CausalMachines::VersionVector";
    let held = exact(ruby, value, classes(ruby)?.vector, message)?;
    let (counters, _) = numbers(ruby, field(held, 0)?)?;
    Ok(Clock::from_counters(counters))
}

/// A `CausalMachines::VersionVector` of `clock`, built through `new`.
pub fn clock_value(ruby: &Ruby, clock: &Clock) -> Result<Value, Error> {
    let class = ruby.get_inner(classes(ruby)?.vector);
    class.funcall("new", (frozen(ruby, clock.counters())?,))
}

pub fn get(ruby: &Ruby, this: Value, slot: Number) -> Result<Option<u64>, Error> {
    Ok(clock(ruby, this)?.get(slot.index()))
}

/// The incremented vector, or the refusal; `self` is a value and stays as is.
pub fn increment(ruby: &Ruby, this: Value, slot: Number) -> Result<Value, Error> {
    let mut vector = clock(ruby, this)?;
    match vector.increment(slot.index()) {
        Ok(_) => clock_value(ruby, &vector),
        Err(why) => Ok(refusal(ruby, why)),
    }
}

pub fn compare(ruby: &Ruby, this: Value, other: Value) -> Result<Value, Error> {
    let name = match clock(ruby, this)?.compare(&clock(ruby, other)?) {
        Causality::Equal => "equal",
        Causality::Before => "before",
        Causality::After => "after",
        Causality::Concurrent => "concurrent",
    };
    Ok(ruby.to_symbol(name).as_value())
}

pub fn dominates(ruby: &Ruby, this: Value, other: Value) -> Result<bool, Error> {
    Ok(clock(ruby, this)?.dominates(&clock(ruby, other)?))
}

pub fn concurrent(ruby: &Ruby, this: Value, other: Value) -> Result<bool, Error> {
    Ok(clock(ruby, this)?.concurrent(&clock(ruby, other)?))
}

pub fn merged(ruby: &Ruby, this: Value, other: Value) -> Result<Value, Error> {
    let merged = clock(ruby, this)?.merged(&clock(ruby, other)?);
    clock_value(ruby, &merged)
}

fn distinct(ruby: &Ruby, ids: &[u64; MEMBERS], len: usize) -> Result<Members, Error> {
    Membership::from_slice(ids.get(..len).unwrap_or(&[]))
        .ok_or_else(|| range_error(ruby, "members must be distinct replica ids"))
}

/// What `Membership.new` stores: the ids `Membership::from_slice` accepts.
pub fn checked_ids(ruby: &Ruby, ids: Value) -> Result<RArray, Error> {
    let (ids, len) = numbers(ruby, ids)?;
    frozen(ruby, distinct(ruby, &ids, len)?.ids())
}

/// The core membership of a `CausalMachines::Membership`.
pub fn members(ruby: &Ruby, value: Value) -> Result<Members, Error> {
    let message = "expected a CausalMachines::Membership";
    let held = exact(ruby, value, classes(ruby)?.membership, message)?;
    let (ids, len) = numbers(ruby, field(held, 0)?)?;
    distinct(ruby, &ids, len)
}

/// A `CausalMachines::Membership` of `members`.
pub fn members_value(ruby: &Ruby, members: &Members) -> Result<Value, Error> {
    let class = ruby.get_inner(classes(ruby)?.membership);
    class.funcall("new", (frozen(ruby, members.ids())?,))
}

pub fn slot(ruby: &Ruby, this: Value, replica: Number) -> Result<Option<usize>, Error> {
    Ok(members(ruby, this)?.slot(replica.0))
}

pub fn replica(ruby: &Ruby, this: Value, slot: Number) -> Result<Option<u64>, Error> {
    Ok(members(ruby, this)?.replica(slot.index()))
}

pub fn len(ruby: &Ruby, this: Value) -> Result<usize, Error> {
    Ok(members(ruby, this)?.len())
}

pub fn is_empty(ruby: &Ruby, this: Value) -> Result<bool, Error> {
    Ok(members(ruby, this)?.is_empty())
}

pub fn admits(ruby: &Ruby, this: Value, vector: Value) -> Result<bool, Error> {
    Ok(members(ruby, this)?.admits(&clock(ruby, vector)?))
}
