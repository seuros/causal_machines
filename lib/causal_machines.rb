# frozen_string_literal: true

require_relative 'causal_machines/version'

module CausalMachines
  # Causal metadata one message travels with: the sender's clock at send time.
  Stamp = Data.define(:generation, :sender, :clock)
  # Causal#release: a held stamp whose dependencies arrived; it is now
  # delivered, so apply its payload. The core's `Release::Deliver(stamp)`.
  Deliver = Data.define(:stamp)
  # Causal#release: a held stamp the delivered clock already covers; drop its
  # payload. The core's `Release::Obsolete(stamp)`.
  Obsolete = Data.define(:stamp)
  # An inclusive run of undelivered counters from one replica.
  Gap = Data.define(:replica, :from, :through)
  # One bounded page of a missing-dependency listing. `gaps` holds the entries
  # the core writes to its output buffer (it reports their count as `written`);
  # `next` is the cursor for the next call, or nil when the listing is done.
  Page = Data.define(:gaps, :next)
end

# Native is required: no algorithm fallback. It builds the Data values above,
# so they must exist first.
require 'causal_machines_native/causal_machines_native'
