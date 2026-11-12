//! Thin binding to the causal-delivery tracker and its version vectors.
mod init;
mod vectors;

use std::cell::RefCell;

use causal_machines::{Causal, Gap, Membership, Readiness, Receipt, Refusal, Release, Stamp};
use magnus::{Error, RArray, RClass, RModule, Ruby, Value, prelude::*, typed_data::Obj};
use vectors::{Clock, Number, array, clock, numbers};

const MEMBERS: usize = 8;
const HOLD: usize = 64;

type Core = Causal<MEMBERS, HOLD>;

fn data_class(ruby: &Ruby, name: &str) -> Result<RClass, Error> {
    let module: RModule = ruby.class_object().const_get("CausalMachines")?;
    module.const_get(name)
}

fn refusal(ruby: &Ruby, refusal: Refusal) -> Value {
    let name = match refusal {
        Refusal::UnknownMember => "unknown_member",
        Refusal::StaleGeneration => "stale_generation",
        Refusal::FutureGeneration => "future_generation",
        Refusal::Exhausted => "exhausted",
        _ => "malformed",
    };
    ruby.to_symbol(name).as_value()
}

#[magnus::wrap(class = "CausalMachines::Tracker", free_immediately, size)]
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
        members: Value,
        local: Number,
        generation: Number,
        delivered: Option<Value>,
    ) -> Result<Self, Error> {
        let (ids, len) = numbers(ruby, members)?;
        let invalid = |message| Error::new(ruby.exception_arg_error(), message);
        let membership = Membership::from_slice(ids.get(..len).unwrap_or(&[]))
            .ok_or_else(|| invalid("members must be distinct"))?;
        let delivered = match delivered {
            Some(value) => clock(ruby, value, len)?,
            None => Clock::new(),
        };
        let core = Causal::restore(membership, local.0, generation.0, delivered)
            .ok_or_else(|| invalid("local must be one of the members"))?;
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    fn stamp(ruby: &Ruby, this: Obj<Self>) -> Result<Value, Error> {
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().stamp();
        match outcome {
            Ok(stamp) => this.wrap(ruby, &stamp),
            Err(why) => Ok(refusal(ruby, why)),
        }
    }

    fn check(ruby: &Ruby, this: Obj<Self>, stamp: Value) -> Result<Value, Error> {
        let stamp = this.unwrap(ruby, stamp)?;
        let name = match this.core.borrow().check(&stamp) {
            Ok(Readiness::Deliverable) => "deliverable",
            Ok(Readiness::Duplicate) => "duplicate",
            Ok(Readiness::Missing) => "missing",
            Err(why) => return Ok(refusal(ruby, why)),
        };
        Ok(ruby.to_symbol(name).as_value())
    }

    fn receive(ruby: &Ruby, this: Obj<Self>, stamp: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let stamp = this.unwrap(ruby, stamp)?;
        let name = match this.core.borrow_mut().receive(&stamp) {
            Ok(Receipt::Delivered) => "delivered",
            Ok(Receipt::Held) => "held",
            Ok(Receipt::AlreadyHeld) => "already_held",
            Ok(Receipt::Duplicate) => "duplicate",
            Ok(Receipt::Conflict) => "conflict",
            Ok(_) => "full",
            Err(why) => return Ok(refusal(ruby, why)),
        };
        Ok(ruby.to_symbol(name).as_value())
    }

    fn release(ruby: &Ruby, this: Obj<Self>) -> Result<Option<RArray>, Error> {
        this.check_frozen()?;
        let released = this.core.borrow_mut().release();
        let Some(released) = released else {
            return Ok(None);
        };
        let (name, stamp) = match released {
            Release::Deliver(stamp) => ("deliver", stamp),
            Release::Obsolete(stamp) => ("obsolete", stamp),
        };
        let pair = ruby.ary_new_capa(2);
        pair.push(ruby.to_symbol(name))?;
        pair.push(this.wrap(ruby, &stamp)?)?;
        Ok(Some(pair))
    }

    fn forget(this: Obj<Self>, sender: Number, counter: Number) -> Result<bool, Error> {
        this.check_frozen()?;
        Ok(this.core.borrow_mut().forget(sender.0, counter.0))
    }

    fn missing(
        ruby: &Ruby,
        this: Obj<Self>,
        stamp: Value,
        cursor: Number,
        limit: Number,
    ) -> Result<Value, Error> {
        let stamp = this.unwrap(ruby, stamp)?;
        let limit = usize::try_from(limit.0).unwrap_or(usize::MAX);
        if limit > MEMBERS {
            return Err(Error::new(
                ruby.exception_range_error(),
                format!("limit is at most {MEMBERS}"),
            ));
        }
        let blank = Gap {
            replica: 0,
            from: 0,
            through: 0,
        };
        let mut gaps = [blank; MEMBERS];
        let cursor = usize::try_from(cursor.0).unwrap_or(usize::MAX);
        let out = gaps.get_mut(..limit).unwrap_or(&mut []);
        let page = match this.core.borrow().missing(&stamp, cursor, out) {
            Ok(page) => page,
            Err(why) => return Ok(refusal(ruby, why)),
        };
        let class = data_class(ruby, "Gap")?;
        let listed = ruby.ary_new_capa(page.written);
        for gap in gaps.iter().take(page.written) {
            listed
                .push(class.funcall::<_, _, Value>("new", (gap.replica, gap.from, gap.through))?)?;
        }
        let answer = ruby.ary_new_capa(2);
        answer.push(listed)?;
        answer.push(page.next)?;
        Ok(answer.as_value())
    }

    fn absorb(
        ruby: &Ruby,
        this: Obj<Self>,
        generation: Number,
        seen: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let seen = clock(ruby, seen, this.len())?;
        Ok(match this.core.borrow_mut().absorb(generation.0, &seen) {
            Ok(()) => ruby.qtrue().as_value(),
            Err(why) => refusal(ruby, why),
        })
    }

    fn clock(ruby: &Ruby, this: Obj<Self>) -> Result<RArray, Error> {
        let core = this.core.borrow();
        array(ruby, core.clock(), core.membership().len())
    }

    fn held(&self) -> usize {
        self.core.borrow().held()
    }

    fn members(ruby: &Ruby, this: Obj<Self>) -> Result<RArray, Error> {
        let out = ruby.ary_new_capa(this.len());
        for id in this.core.borrow().membership().ids() {
            out.push(*id)?;
        }
        Ok(out)
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

    fn len(&self) -> usize {
        self.core.borrow().membership().len()
    }

    fn wrap(&self, ruby: &Ruby, stamp: &Stamp<MEMBERS>) -> Result<Value, Error> {
        let counters = array(ruby, &stamp.clock, self.len())?;
        data_class(ruby, "Stamp")?.funcall("new", (stamp.generation, stamp.sender, counters))
    }

    fn unwrap(&self, ruby: &Ruby, value: Value) -> Result<Stamp<MEMBERS>, Error> {
        if !value.is_kind_of(data_class(ruby, "Stamp")?) {
            return Err(Error::new(
                ruby.exception_type_error(),
                "expected a CausalMachines::Stamp",
            ));
        }
        let generation: Number = value.funcall("generation", ())?;
        let sender: Number = value.funcall("sender", ())?;
        let counters: Value = value.funcall("clock", ())?;
        let seen: Clock = clock(ruby, counters, self.len())?;
        Ok(Stamp::new(generation.0, sender.0, seen))
    }
}
