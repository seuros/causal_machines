//! Conversions between Ruby arrays and version vectors, and the module-level
//! vector operations.
use causal_machines::{Causality, VersionVector};
use magnus::{Error, Integer, RArray, Ruby, TryConvert, Value, prelude::*};

use crate::MEMBERS;

pub type Clock = VersionVector<MEMBERS>;

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

/// The Integers of a Ruby Array of at most [`MEMBERS`] entries.
pub fn numbers(ruby: &Ruby, value: Value) -> Result<([u64; MEMBERS], usize), Error> {
    let Some(array) = RArray::from_value(value) else {
        return Err(Error::new(
            ruby.exception_type_error(),
            "expected an Array of Integers",
        ));
    };
    let len = array.len();
    if len > MEMBERS {
        return Err(Error::new(
            ruby.exception_range_error(),
            format!("at most {MEMBERS} entries fit"),
        ));
    }
    let mut out = [0; MEMBERS];
    for (index, slot) in out.iter_mut().enumerate().take(len) {
        let position = isize::try_from(index).unwrap_or(isize::MAX);
        *slot = array.entry::<Number>(position)?.0;
    }
    Ok((out, len))
}

/// A clock of exactly `len` counters.
pub fn clock(ruby: &Ruby, value: Value, len: usize) -> Result<Clock, Error> {
    let (counters, given) = numbers(ruby, value)?;
    if given != len {
        return Err(Error::new(
            ruby.exception_range_error(),
            format!("expected {len} counters, got {given}"),
        ));
    }
    Ok(Clock::from_counters(counters))
}

/// The first `len` counters as a Ruby Array.
pub fn array(ruby: &Ruby, clock: &Clock, len: usize) -> Result<RArray, Error> {
    let out = ruby.ary_new_capa(len);
    for counter in clock.counters().iter().take(len) {
        out.push(*counter)?;
    }
    Ok(out)
}

fn pair(ruby: &Ruby, left: Value, right: Value) -> Result<(Clock, Clock, usize), Error> {
    let (counters, len) = numbers(ruby, left)?;
    Ok((
        Clock::from_counters(counters),
        clock(ruby, right, len)?,
        len,
    ))
}

pub fn compare(ruby: &Ruby, left: Value, right: Value) -> Result<Value, Error> {
    let (left, right, _) = pair(ruby, left, right)?;
    let name = match left.compare(&right) {
        Causality::Equal => "equal",
        Causality::Before => "before",
        Causality::After => "after",
        Causality::Concurrent => "concurrent",
    };
    Ok(ruby.to_symbol(name).as_value())
}

pub fn dominates(ruby: &Ruby, left: Value, right: Value) -> Result<bool, Error> {
    let (left, right, _) = pair(ruby, left, right)?;
    Ok(left.dominates(&right))
}

pub fn concurrent(ruby: &Ruby, left: Value, right: Value) -> Result<bool, Error> {
    let (left, right, _) = pair(ruby, left, right)?;
    Ok(left.concurrent(&right))
}

pub fn merge(ruby: &Ruby, left: Value, right: Value) -> Result<RArray, Error> {
    let (left, right, len) = pair(ruby, left, right)?;
    array(ruby, &left.merged(&right), len)
}
