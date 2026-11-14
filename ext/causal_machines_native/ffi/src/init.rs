//! Private native entry point.
use crate::{HOLD, MEMBERS, Tracker, classes, values, vectors};
use magnus::{Error, RModule, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("CausalMachines")?;
    module.const_set("MEMBERS", MEMBERS)?;
    module.const_set("CAPACITY", HOLD)?;
    hooks(module)?;
    let classes = classes::resolve(ruby, module)?;
    let vector = ruby.get_inner(classes.vector);
    vector.define_method("get", method!(vectors::get, 1))?;
    vector.define_method("increment", method!(vectors::increment, 1))?;
    vector.define_method("compare", method!(vectors::compare, 1))?;
    vector.define_method("dominates?", method!(vectors::dominates, 1))?;
    vector.define_method("concurrent?", method!(vectors::concurrent, 1))?;
    vector.define_method("merged", method!(vectors::merged, 1))?;
    let membership = ruby.get_inner(classes.membership);
    membership.define_method("slot", method!(vectors::slot, 1))?;
    membership.define_method("replica", method!(vectors::replica, 1))?;
    membership.define_method("len", method!(vectors::len, 0))?;
    membership.define_method("empty?", method!(vectors::is_empty, 0))?;
    membership.define_method("admits?", method!(vectors::admits, 1))?;
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

/// The private validating hooks the `Data` values call from `initialize`.
fn hooks(module: RModule) -> Result<(), Error> {
    module.define_singleton_method("__counters", function!(vectors::checked_counters, 1))?;
    module.define_singleton_method("__ids", function!(vectors::checked_ids, 1))?;
    module.define_singleton_method("__stamp", function!(values::checked_stamp, 3))?;
    module.define_singleton_method("__release", function!(values::checked_release, 1))?;
    module.define_singleton_method("__gap", function!(values::checked_gap_fields, 3))?;
    module.define_singleton_method("__page", function!(values::checked_page, 2))?;
    Ok(())
}
