//! The `VersionVector` and `Membership` values: their validating hooks, the
//! conversions to and from the core types, and their methods.
use causal_machines::{Causality, Membership, VersionVector};
use magnus::{Error, RArray, Ruby, Value, prelude::*};
use matryona_machines::ruby::{Symbols, exact_array, exact_fields, integer};

use crate::classes::classes;
use crate::{MEMBERS, range_error, refusal};

pub type Clock = VersionVector<MEMBERS>;
pub type Members = Membership<MEMBERS>;

/// The Integers of a Ruby Array of at most [`MEMBERS`] entries, `name`
/// naming each in a conversion error.
fn numbers(ruby: &Ruby, value: Value, name: &str) -> Result<([u64; MEMBERS], usize), Error> {
    let array = exact_array(ruby, value, "expected an Array of Integers")?;
    let len = array.len();
    if len > MEMBERS {
        return Err(range_error(ruby, format!("at most {MEMBERS} entries fit")));
    }
    let mut out = [0; MEMBERS];
    for (index, slot) in out.iter_mut().enumerate().take(len) {
        *slot = integer(entry(array, index)?, name)?;
    }
    Ok((out, len))
}

/// Entry `index` of `array`; past `isize::MAX` reads as past the end.
pub fn entry(array: RArray, index: usize) -> Result<Value, Error> {
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
    let (counters, _) = numbers(ruby, counters, "counter")?;
    frozen(ruby, Clock::from_counters(counters).counters())
}

/// The core vector of a `CausalMachines::VersionVector`.
pub fn clock(ruby: &Ruby, value: Value) -> Result<Clock, Error> {
    let message = "expected a CausalMachines::VersionVector";
    let [counters] = exact_fields(ruby, value, classes(ruby)?.vector.get(ruby), message)?;
    let (counters, _) = numbers(ruby, counters, "counter")?;
    Ok(Clock::from_counters(counters))
}

/// A `CausalMachines::VersionVector` of `clock`, built through `new`.
pub fn clock_value(ruby: &Ruby, clock: &Clock) -> Result<Value, Error> {
    let class = classes(ruby)?.vector.get(ruby);
    class.funcall("new", (frozen(ruby, clock.counters())?,))
}

pub fn get(ruby: &Ruby, this: Value, slot: Value) -> Result<Option<u64>, Error> {
    let slot = integer(slot, "slot")?;
    Ok(clock(ruby, this)?.get(slot))
}

/// The incremented vector, or the refusal; `self` is a value and stays as is.
pub fn increment(ruby: &Ruby, this: Value, slot: Value) -> Result<Value, Error> {
    let slot = integer(slot, "slot")?;
    let mut vector = clock(ruby, this)?;
    match vector.increment(slot) {
        Ok(_) => clock_value(ruby, &vector),
        Err(why) => Ok(refusal(ruby, why)),
    }
}

matryona_machines::symbols! {
    /// What `compare` answers.
    Causalities for Causality as "a causality" {
        Equal => "equal",
        Before => "before",
        After => "after",
        Concurrent => "concurrent",
    }
}

pub fn compare(ruby: &Ruby, this: Value, other: Value) -> Result<Value, Error> {
    let causality = clock(ruby, this)?.compare(&clock(ruby, other)?);
    Ok(Causalities::symbol(ruby, causality).as_value())
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
    let (ids, len) = numbers(ruby, ids, "id")?;
    frozen(ruby, distinct(ruby, &ids, len)?.ids())
}

/// The core membership of a `CausalMachines::Membership`.
pub fn members(ruby: &Ruby, value: Value) -> Result<Members, Error> {
    let message = "expected a CausalMachines::Membership";
    let [ids] = exact_fields(ruby, value, classes(ruby)?.membership.get(ruby), message)?;
    let (ids, len) = numbers(ruby, ids, "id")?;
    distinct(ruby, &ids, len)
}

/// A `CausalMachines::Membership` of `members`.
pub fn members_value(ruby: &Ruby, members: &Members) -> Result<Value, Error> {
    let class = classes(ruby)?.membership.get(ruby);
    class.funcall("new", (frozen(ruby, members.ids())?,))
}

pub fn slot(ruby: &Ruby, this: Value, replica: Value) -> Result<Option<usize>, Error> {
    let replica = integer(replica, "replica")?;
    Ok(members(ruby, this)?.slot(replica))
}

pub fn replica(ruby: &Ruby, this: Value, slot: Value) -> Result<Option<u64>, Error> {
    let slot = integer(slot, "slot")?;
    Ok(members(ruby, this)?.replica(slot))
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
