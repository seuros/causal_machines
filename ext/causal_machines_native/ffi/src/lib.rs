//! Thin binding to the causal-delivery tracker and its version vectors.
mod classes;
mod init;
mod values;
mod vectors;

use std::cell::RefCell;

use causal_machines::{Absorbing, Causal, Forgetting, Gap, Readiness, Receipt, Refusal, Release};
use classes::{classes, keep};
use magnus::{Error, Ruby, Value, prelude::*, typed_data::Obj};
use values::{gap_value, stamp_value};
use vectors::{Clock, Number, clock, clock_value, members, members_value};

const MEMBERS: usize = 8;
const HOLD: usize = 64;

type Core = Causal<MEMBERS, HOLD>;

/// A `RangeError`: right type, a value the core does not accept.
fn range_error(ruby: &Ruby, message: impl Into<std::borrow::Cow<'static, str>>) -> Error {
    Error::new(ruby.exception_range_error(), message)
}

/// The snake case of the core variant.
fn refusal(ruby: &Ruby, refusal: Refusal) -> Value {
    let name = match refusal {
        Refusal::UnknownMember => "unknown_member",
        Refusal::StaleGeneration => "stale_generation",
        Refusal::FutureGeneration => "future_generation",
        Refusal::Exhausted => "exhausted",
        Refusal::Conflict => "conflict",
        Refusal::Malformed => "malformed",
        Refusal::Full => "full",
    };
    ruby.to_symbol(name).as_value()
}

/// The symbol naming an honoured outcome, or the refusal's symbol.
fn answer(ruby: &Ruby, outcome: Result<&str, Refusal>) -> Value {
    match outcome {
        Ok(name) => ruby.to_symbol(name).as_value(),
        Err(why) => refusal(ruby, why),
    }
}

#[magnus::wrap(class = "CausalMachines::Causal", free_immediately, size)]
struct Tracker {
    core: RefCell<Core>,
}

impl Tracker {
    fn new(ruby: &Ruby, members: Value, local: Number, generation: Number) -> Result<Self, Error> {
        Self::build(ruby, members, local, generation, None)
    }

    fn restore(
        ruby: &Ruby,
        members: Value,
        local: Number,
        generation: Number,
        delivered: Value,
    ) -> Result<Self, Error> {
        Self::build(ruby, members, local, generation, Some(delivered))
    }

    fn build(
        ruby: &Ruby,
        membership: Value,
        local: Number,
        generation: Number,
        delivered: Option<Value>,
    ) -> Result<Self, Error> {
        let membership = members(ruby, membership)?;
        let delivered = match delivered {
            Some(value) => clock(ruby, value)?,
            None => Clock::new(),
        };
        let core =
            Causal::restore(membership, local.0, generation.0, delivered).ok_or_else(|| {
                range_error(
                    ruby,
                    "local must be a member, and delivered must count member slots only",
                )
            })?;
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    /// Clone-then-keep: the answer is a `Stamp` built through `new`.
    fn stamp(ruby: &Ruby, this: Obj<Self>) -> Result<Value, Error> {
        keep(
            ruby,
            this,
            &this.core,
            Core::stamp,
            |outcome| match outcome {
                Ok(stamp) => stamp_value(ruby, &stamp),
                Err(why) => Ok(refusal(ruby, why)),
            },
        )
    }

    fn check(ruby: &Ruby, this: Obj<Self>, stamp: Value) -> Result<Value, Error> {
        let stamp = values::stamp(ruby, stamp)?;
        let outcome = this.core.borrow().check(&stamp);
        Ok(answer(
            ruby,
            outcome.map(|readiness| match readiness {
                Readiness::Deliverable => "deliverable",
                Readiness::Duplicate => "duplicate",
                Readiness::Missing => "missing",
            }),
        ))
    }

    fn receive(ruby: &Ruby, this: Obj<Self>, stamp: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let stamp = values::stamp(ruby, stamp)?;
        let outcome = this.core.borrow_mut().receive(&stamp);
        Ok(answer(
            ruby,
            outcome.map(|receipt| match receipt {
                Receipt::Delivered => "delivered",
                Receipt::Held => "held",
                Receipt::AlreadyHeld => "already_held",
                Receipt::Duplicate => "duplicate",
            }),
        ))
    }

    /// A `Deliver` or an `Obsolete` wrapping the stamp, or `nil`.
    /// Clone-then-keep: the stamp leaves the hold table only once its answer
    /// is built.
    fn release(ruby: &Ruby, this: Obj<Self>) -> Result<Value, Error> {
        keep(ruby, this, &this.core, Core::release, |released| {
            let Some(released) = released else {
                return Ok(ruby.qnil().as_value());
            };
            let classes = classes(ruby)?;
            let (class, stamp) = match released {
                Release::Deliver(stamp) => (classes.deliver, stamp),
                Release::Obsolete(stamp) => (classes.obsolete, stamp),
            };
            let stamp = stamp_value(ruby, &stamp)?;
            ruby.get_inner(class).funcall("new", (stamp,))
        })
    }

    fn forget(
        ruby: &Ruby,
        this: Obj<Self>,
        sender: Number,
        counter: Number,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().forget(sender.0, counter.0);
        Ok(answer(
            ruby,
            outcome.map(|forgetting| match forgetting {
                Forgetting::Forgotten => "forgotten",
                Forgetting::NotHeld => "not_held",
            }),
        ))
    }

    fn missing(
        ruby: &Ruby,
        this: Obj<Self>,
        stamp: Value,
        cursor: Number,
        limit: Number,
    ) -> Result<Value, Error> {
        let stamp = values::stamp(ruby, stamp)?;
        let limit = limit.index();
        if limit > MEMBERS {
            return Err(range_error(ruby, format!("limit is at most {MEMBERS}")));
        }
        let mut gaps = [Gap::default(); MEMBERS];
        let out = gaps.get_mut(..limit).unwrap_or(&mut []);
        let outcome = this.core.borrow().missing(&stamp, cursor.index(), out);
        let page = match outcome {
            Ok(page) => page,
            Err(why) => return Ok(refusal(ruby, why)),
        };
        let listed = ruby.ary_new_capa(page.written());
        for gap in gaps.iter().take(page.written()) {
            listed.push(gap_value(ruby, gap)?)?;
        }
        let class = ruby.get_inner(classes(ruby)?.page);
        class.funcall("new", (listed, page.next()))
    }

    fn absorb(
        ruby: &Ruby,
        this: Obj<Self>,
        generation: Number,
        seen: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let seen = clock(ruby, seen)?;
        let outcome = this.core.borrow_mut().absorb(generation.0, &seen);
        Ok(answer(
            ruby,
            outcome.map(|absorbing| match absorbing {
                Absorbing::Absorbed => "absorbed",
                Absorbing::AlreadyCovered => "already_covered",
            }),
        ))
    }

    fn clock(ruby: &Ruby, this: Obj<Self>) -> Result<Value, Error> {
        let delivered = *this.core.borrow().clock();
        clock_value(ruby, &delivered)
    }

    fn held(&self) -> usize {
        self.core.borrow().held()
    }

    fn membership(ruby: &Ruby, this: Obj<Self>) -> Result<Value, Error> {
        let membership = *this.core.borrow().membership();
        members_value(ruby, &membership)
    }

    fn local(&self) -> u64 {
        self.core.borrow().local()
    }

    fn generation(&self) -> u64 {
        self.core.borrow().generation()
    }

    const fn capacity() -> usize {
        HOLD
    }
}
