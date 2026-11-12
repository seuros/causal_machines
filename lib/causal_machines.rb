# frozen_string_literal: true

require_relative 'causal_machines/version'

module CausalMachines
  # Causal metadata one message travels with: the sender's clock at send time.
  Stamp = Data.define(:generation, :sender, :clock)
  # An inclusive run of undelivered counters from one replica.
  Gap = Data.define(:replica, :from, :through)
end

# Native is required: no algorithm fallback.
require 'causal_machines_native/causal_machines_native'
