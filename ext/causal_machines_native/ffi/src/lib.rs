//! Thin binding to the causal-delivery tracker and its version vectors.
mod classes;
mod init;
mod values;
mod vectors;

use std::cell::RefCell;

use causal_machines::{Absorbing, Causal, Forgetting, Gap, Readiness, Receipt, Refusal, Release};
use classes::classes;
use magnus::{Error, Ruby, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{Symbols, integer, integer_in, keep};
use values::{gap_value, stamp_value};
use vectors::{Clock, clock, clock_value, members, members_value};

const MEMBERS: usize = 8;
const HOLD: usize = 64;

type Core = Causal<MEMBERS, HOLD>;

/// A `RangeError`: right type, a value the core does not accept.
fn range_error(ruby: &Ruby, message: impl Into<std::borrow::Cow<'static, str>>) -> Error {
    Error::new(ruby.exception_range_error(), message)
}

matryona_machines::symbols! {
    /// The core's refusals, each the snake case of its variant.
    pub(crate) Refusals for Refusal as "a refusal" {
        UnknownMember => "unknown_member",
        StaleGeneration => "stale_generation",
        FutureGeneration => "future_generation",
        Exhausted => "exhausted",
        Conflict => "conflict",
        Malformed => "malformed",
        Full => "full",
    }
}

matryona_machines::symbols! {
    /// What `check` answers.
    Readinesses for Readiness as "a readiness" {
        Deliverable => "deliverable",
        Duplicate => "duplicate",
        Missing => "missing",
    }
}

matryona_machines::symbols! {
    /// What `receive` answers.
    Receipts for Receipt as "a receipt" {
        Delivered => "delivered",
        Held => "held",
        AlreadyHeld => "already_held",
        Duplicate => "duplicate",
    }
}

matryona_machines::symbols! {
    /// What `forget` answers.
    Forgettings for Forgetting as "a forgetting" {
        Forgotten => "forgotten",
        NotHeld => "not_held",
    }
}

matryona_machines::symbols! {
    /// What `absorb` answers.
    Absorbings for Absorbing as "an absorbing" {
        Absorbed => "absorbed",
        AlreadyCovered => "already_covered",
    }
}

/// The refusal's symbol.
fn refusal(ruby: &Ruby, refusal: Refusal) -> Value {
    Refusals::symbol(ruby, refusal).as_value()
}

/// The symbol naming an honoured outcome through its table `T`, or the
/// refusal's symbol.
fn answer<T: Symbols>(ruby: &Ruby, outcome: Result<T::Variant, Refusal>) -> Value {
    match outcome {
        Ok(honoured) => T::symbol(ruby, honoured).as_value(),
        Err(why) => refusal(ruby, why),
    }
}

#[magnus::wrap(class = "CausalMachines::Causal", free_immediately, size)]
struct Tracker {
    core: RefCell<Core>,
}

impl Tracker {
    fn new(ruby: &Ruby, members: Value, local: Value, generation: Value) -> Result<Self, Error> {
        Self::build(ruby, members, local, generation, None)
    }

    fn restore(
        ruby: &Ruby,
        members: Value,
        local: Value,
        generation: Value,
        delivered: Value,
    ) -> Result<Self, Error> {
        Self::build(ruby, members, local, generation, Some(delivered))
    }

    fn build(
        ruby: &Ruby,
        membership: Value,
        local: Value,
        generation: Value,
        delivered: Option<Value>,
    ) -> Result<Self, Error> {
        let (local, generation) = (integer(local, "local")?, integer(generation, "generation")?);
        let membership = members(ruby, membership)?;
        let delivered = match delivered {
            Some(value) => clock(ruby, value)?,
            None => Clock::new(),
        };
        let core = Causal::restore(membership, local, generation, delivered).ok_or_else(|| {
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
        Ok(answer::<Readinesses>(ruby, outcome))
    }

    fn receive(ruby: &Ruby, this: Obj<Self>, stamp: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let stamp = values::stamp(ruby, stamp)?;
        let outcome = this.core.borrow_mut().receive(&stamp);
        Ok(answer::<Receipts>(ruby, outcome))
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
            class.get(ruby).funcall("new", (stamp,))
        })
    }

    fn forget(ruby: &Ruby, this: Obj<Self>, sender: Value, counter: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let (sender, counter) = (integer(sender, "sender")?, integer(counter, "counter")?);
        let outcome = this.core.borrow_mut().forget(sender, counter);
        Ok(answer::<Forgettings>(ruby, outcome))
    }

    fn missing(
        ruby: &Ruby,
        this: Obj<Self>,
        stamp: Value,
        cursor: Value,
        limit: Value,
    ) -> Result<Value, Error> {
        let cursor: usize = integer(cursor, "cursor")?;
        let limit = integer_in(limit, "limit", 0..=MEMBERS)?;
        let stamp = values::stamp(ruby, stamp)?;
        let mut gaps = [Gap::default(); MEMBERS];
        let out = gaps.get_mut(..limit).unwrap_or(&mut []);
        let outcome = this.core.borrow().missing(&stamp, cursor, out);
        let page = match outcome {
            Ok(page) => page,
            Err(why) => return Ok(refusal(ruby, why)),
        };
        let listed = ruby.ary_new_capa(page.written());
        for gap in gaps.iter().take(page.written()) {
            listed.push(gap_value(ruby, gap)?)?;
        }
        let class = classes(ruby)?.page.get(ruby);
        class.funcall("new", (listed, page.next()))
    }

    fn absorb(
        ruby: &Ruby,
        this: Obj<Self>,
        generation: Value,
        seen: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let generation = integer(generation, "generation")?;
        let seen = clock(ruby, seen)?;
        let outcome = this.core.borrow_mut().absorb(generation, &seen);
        Ok(answer::<Absorbings>(ruby, outcome))
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
