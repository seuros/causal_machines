# frozen_string_literal: true

require_relative 'causal_machines/version'

# Every value below validates at construction through the core: a wrong type
# raises TypeError, a value the core would not build raises RangeError. A value
# built by hand is held to the same rule as one the gem returned.
module CausalMachines
  # The core's `VersionVector<MEMBERS>`: one counter per membership slot.
  # `VersionVector.new` is the core's `new()` (nothing seen) and
  # `VersionVector.new(counters)` its `from_counters`: at most MEMBERS
  # Integers in 0..2**64-1; slots not given count zero, so `counters` always
  # holds MEMBERS entries. Methods (native): get, increment, compare,
  # dominates?, concurrent?, merged.
  VersionVector = Data.define(:counters) do
    def initialize(counters: [])
      super(counters: CausalMachines.__send__(:__counters, counters))
    end
  end

  # The core's `Membership<MEMBERS>`: up to MEMBERS distinct replica ids,
  # each bound to the slot of its position, as `Membership::from_slice`
  # accepts them (duplicates or too many raise RangeError). Methods (native):
  # slot, replica, len, empty?, admits?.
  Membership = Data.define(:ids) do
    def initialize(ids:)
      super(ids: CausalMachines.__send__(:__ids, ids))
    end
  end

  # Causal metadata one message travels with: the sender's VersionVector at
  # send time. Like the core's `Stamp::new`, any generation and sender build
  # one; the receiving tracker judges it.
  Stamp = Data.define(:generation, :sender, :clock) do
    def initialize(generation:, sender:, clock:)
      generation, sender, clock = CausalMachines.__send__(:__stamp, generation, sender, clock)
      super
    end
  end

  # Causal#release: a held stamp whose dependencies arrived; it is now
  # delivered, so apply its payload. The core's `Release::Deliver(stamp)`.
  Deliver = Data.define(:stamp) do
    def initialize(stamp:)
      super(stamp: CausalMachines.__send__(:__release, stamp))
    end
  end

  # Causal#release: a held stamp the delivered clock already covers; drop its
  # payload. The core's `Release::Obsolete(stamp)`.
  Obsolete = Data.define(:stamp) do
    def initialize(stamp:)
      super(stamp: CausalMachines.__send__(:__release, stamp))
    end
  end

  # An inclusive run of undelivered counters from one replica, as `Gap::new`
  # accepts it: 1 <= from <= through.
  Gap = Data.define(:replica, :from, :through) do
    def initialize(replica:, from:, through:)
      replica, from, through = CausalMachines.__send__(:__gap, replica, from, through)
      super
    end
  end

  # One bounded page of a missing-dependency listing. `gaps` holds the entries
  # the core writes to its output buffer (it reports their count as `written`),
  # at most MEMBERS; `next` is the cursor for the next call, a slot below
  # MEMBERS, or nil when the listing is done.
  Page = Data.define(:gaps, :next) do
    def initialize(gaps:, next:)
      gaps, cursor = CausalMachines.__send__(:__page, gaps, binding.local_variable_get(:next))
      super(gaps: gaps, next: cursor)
    end
  end
end

# Native is required: no algorithm fallback. It validates and builds the Data
# values above, so they must exist first.
require 'causal_machines_native/causal_machines_native'

module CausalMachines
  private_class_method :__counters, :__ids, :__stamp, :__release, :__gap, :__page

  # The values are final. `initialize` is where each validates, and a subclass
  # could override it, or `allocate` build an instance that never ran it:
  # subclassing raises TypeError (as subclassing Class does) and `allocate`
  # is gone. `new`, keywords, `with` and `dup` all still run the check.
  [VersionVector, Membership, Stamp, Deliver, Obsolete, Gap, Page].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end
end
