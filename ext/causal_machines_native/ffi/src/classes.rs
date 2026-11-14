//! The `Data` classes, resolved once at load and pinned. The exact-class
//! reads and clone-then-keep are `matryona_machines::ruby`'s.
//!
//! A value read back must be an instance of exactly the gem's class: a
//! subclass constant leaks past the seal and `Data#initialize` can be bound
//! directly, so `is_a?` is not enough. Its fields are read straight from the
//! struct (no reader runs) and rebuilt through the core constructor, which
//! `Marshal.load` skipped.
use magnus::{Error, RModule, Ruby};
use matryona_machines::ruby::{Class, Registry};

/// Every `Data` class the binding checks or builds.
pub struct Classes {
    pub vector: Class,
    pub membership: Class,
    pub stamp: Class,
    pub deliver: Class,
    pub obsolete: Class,
    pub gap: Class,
    pub page: Class,
}

static CLASSES: Registry<Classes> = Registry::new("causal_machines");

/// Resolve the classes `causal_machines.rb` defined before requiring the
/// extension.
pub fn resolve(ruby: &Ruby, module: RModule) -> Result<&'static Classes, Error> {
    let class = |name| Class::resolve(ruby, module, name);
    let resolved = Classes {
        vector: class("VersionVector")?,
        membership: class("Membership")?,
        stamp: class("Stamp")?,
        deliver: class("Deliver")?,
        obsolete: class("Obsolete")?,
        gap: class("Gap")?,
        page: class("Page")?,
    };
    CLASSES.set(ruby, resolved)
}

/// The classes resolved at load.
pub fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get(ruby)
}
