# frozen_string_literal: true

require 'minitest/autorun'
require 'causal_machines'

# A stamp that conflicts with a held one is refused on every path.
class ConflictTest < Minitest::Test
  include CausalMachines

  def setup
    @node = Causal.new(Membership.new([1, 2, 3]), 3, 1)
    @held = stamp([2, 1, 0])
    assert_equal :held, @node.receive(@held)
  end

  def vv(counters) = VersionVector.new(counters)

  def stamp(clock) = Stamp.new(1, 1, vv(clock))

  def test_a_conflicting_stamp_is_refused_after_the_gap_fills
    assert_equal :delivered, @node.receive(stamp([1, 0, 0]))
    other = stamp([2, 0, 0])
    assert_equal :conflict, @node.check(other)
    assert_equal :conflict, @node.receive(other)
    assert_equal :conflict, @node.missing(other, 0, 8)
    assert_equal [1, [1, 0, 0]], [@node.held, @node.clock.counters.first(3)]
    assert_equal :already_held, @node.receive(@held)
    assert_equal :absorbed, @node.absorb(1, vv([0, 1, 0]))
    assert_equal Deliver.new(@held), @node.release
  end

  def test_a_conflicting_stamp_is_refused_while_it_would_wait
    other = stamp([2, 0, 1])
    assert_equal :conflict, @node.check(other)
    assert_equal :conflict, @node.receive(other)
    assert_equal 1, @node.held
  end

  def test_a_conflicting_stamp_is_refused_once_the_counter_is_covered
    assert_equal :absorbed, @node.absorb(1, vv([2, 0, 0]))
    other = stamp([2, 0, 0])
    assert_equal :conflict, @node.check(other)
    assert_equal :conflict, @node.receive(other)
    assert_equal :duplicate, @node.receive(@held)
    assert_equal Obsolete.new(@held), @node.release
    assert_equal :duplicate, @node.receive(other)
  end

  def test_forgetting_the_held_stamp_resolves_the_conflict
    assert_equal :delivered, @node.receive(stamp([1, 0, 0]))
    other = stamp([2, 0, 0])
    assert_equal :conflict, @node.receive(other)
    assert_equal :forgotten, @node.forget(1, 2)
    assert_equal :delivered, @node.receive(other)
    assert_equal [0, [2, 0, 0]], [@node.held, @node.clock.counters.first(3)]
  end
end
