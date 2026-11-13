//! Private native entry point.
use crate::{HOLD, MEMBERS, Tracker, vectors};
use magnus::{Error, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("CausalMachines")?;
    module.const_set("MEMBERS", MEMBERS)?;
    module.const_set("CAPACITY", HOLD)?;
    module.define_singleton_method("compare", function!(vectors::compare, 2))?;
    module.define_singleton_method("dominates?", function!(vectors::dominates, 2))?;
    module.define_singleton_method("concurrent?", function!(vectors::concurrent, 2))?;
    module.define_singleton_method("merge", function!(vectors::merge, 2))?;
    let class = module.define_class("Causal", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Tracker::new, 3))?;
    class.define_singleton_method("restore", function!(Tracker::restore, 4))?;
    class.define_method("stamp", method!(Tracker::stamp, 0))?;
    class.define_method("check", method!(Tracker::check, 1))?;
    class.define_method("receive", method!(Tracker::receive, 1))?;
    class.define_method("release", method!(Tracker::release, 0))?;
    class.define_method("forget", method!(Tracker::forget, 2))?;
    class.define_method("missing", method!(Tracker::missing, 3))?;
    class.define_method("absorb", method!(Tracker::absorb, 2))?;
    class.define_method("clock", method!(Tracker::clock, 0))?;
    class.define_method("held", method!(Tracker::held, 0))?;
    class.define_method("membership", method!(Tracker::membership, 0))?;
    class.define_method("local", method!(Tracker::local, 0))?;
    class.define_method("generation", method!(Tracker::generation, 0))?;
    class.define_method("capacity", function!(Tracker::capacity, 0))?;
    Ok(())
}
