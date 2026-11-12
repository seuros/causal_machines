# frozen_string_literal: true
require 'minitest/autorun'
require 'causal_machines'

class TrackerTest < Minitest::Test
  include CausalMachines

  MAX = 2**64 - 1

  def tracker(local, members: [1, 2, 3], generation: 0)
    Tracker.new(members, local, generation)
  end

  def stamp(sender, clock, generation: 0)
    Stamp.new(generation, sender, clock)
  end

  def test_configuration_and_constants
    node = tracker(2, generation: 9)
    assert_equal 8, MEMBERS
    assert_equal 64, CAPACITY
    assert_equal 64, node.capacity
    assert_equal [1, 2, 3], node.members
    assert_equal 2, node.local
    assert_equal 9, node.generation
    assert_equal [0, 0, 0], node.clock
    assert_equal 0, node.held
  end

  def test_causal_order_survives_reordering
    alice = tracker(1)
    bob = tracker(2)
    carol = tracker(3)
    question = alice.stamp
    assert_equal Stamp.new(0, 1, [1, 0, 0]), question
    assert_equal :delivered, bob.receive(question)
    answer = bob.stamp
    assert_equal [1, 1, 0], answer.clock

    assert_equal :missing, carol.check(answer)
    assert_equal :held, carol.receive(answer)
    assert_equal :already_held, carol.receive(answer)
    assert_equal 1, carol.held
    assert_nil carol.release
    assert_equal [[Gap.new(1, 1, 1)], nil], carol.missing(answer, 0, 8)
    assert_equal :deliverable, carol.check(question)
    assert_equal :delivered, carol.receive(question)
    assert_equal [:deliver, answer], carol.release
    assert_nil carol.release
    assert_equal [1, 1, 0], carol.clock
    assert_equal :duplicate, carol.receive(question)
    assert_equal :duplicate, carol.check(answer)
    assert_equal :duplicate, alice.receive(question)
  end

  def test_missing_pages_resume_from_the_cursor
    node = tracker(3)
    wanted = stamp(2, [3, 4, 2])
    first, cursor = node.missing(wanted, 0, 2)
    assert_equal [Gap.new(1, 1, 3), Gap.new(2, 1, 3)], first
    assert_equal 2, cursor
    assert_equal [[Gap.new(3, 1, 2)], nil], node.missing(wanted, cursor, 2)
    assert_equal [[], 0], node.missing(wanted, 0, 0)
    assert_equal [[], nil], node.missing(stamp(1, [1, 0, 0]), 0, 8)
    assert_raises(RangeError) { node.missing(wanted, 0, 9) }
  end

  def test_full_conflict_and_forget
    node = tracker(3)
    (2..65).each { |n| assert_equal :held, node.receive(stamp(1, [n, 0, 0])) }
    assert_equal 64, node.held
    assert_equal :full, node.receive(stamp(1, [66, 0, 0]))
    assert_equal :conflict, node.receive(stamp(1, [2, 1, 0]))
    assert_equal 64, node.held
    assert_equal [0, 0, 0], node.clock
    assert node.forget(1, 65)
    refute node.forget(1, 65)
    refute node.forget(99, 1)
    assert_equal :held, node.receive(stamp(1, [66, 0, 0]))
    assert_equal :delivered, node.receive(stamp(1, [1, 0, 0]))
    released = []
    while (out = node.release)
      released << out
    end
    assert_equal((2..64).map { |n| [:deliver, stamp(1, [n, 0, 0])] }, released)
    assert_equal 1, node.held
  end

  def test_absorb_and_obsolete_release
    node = tracker(3)
    late = stamp(1, [2, 0, 0])
    assert_equal :held, node.receive(late)
    assert_equal true, node.absorb(0, [4, 0, 0])
    assert_equal [4, 0, 0], node.clock
    assert_equal [:obsolete, late], node.release
    assert_nil node.release
    assert_equal :stale_generation, tracker(3, generation: 2).absorb(1, [1, 0, 0])
    assert_equal :future_generation, node.absorb(1, [1, 0, 0])
    assert_raises(RangeError) { node.absorb(0, [1, 0]) }
  end

  def test_refusals_are_symbols_and_change_nothing
    node = tracker(3, generation: 5)
    good = [1, 0, 0]
    {
      stamp(1, good, generation: 4) => :stale_generation,
      stamp(1, good, generation: 6) => :future_generation,
      stamp(9, good, generation: 5) => :unknown_member,
      stamp(1, [0, 1, 0], generation: 5) => :malformed
    }.each do |bad, why|
      assert_equal why, node.check(bad)
      assert_equal why, node.receive(bad)
      assert_equal why, node.missing(bad, 0, 8)
    end
    assert_equal [0, 0, 0], node.clock
    assert_equal 0, node.held
  end

  def test_restore_and_u64_limits
    node = Tracker.restore([1, 2], 1, 0, [MAX - 1, 0])
    last = node.stamp
    assert_equal [MAX, 0], last.clock
    assert_equal :exhausted, node.stamp
    assert_equal [MAX, 0], node.clock
    peer = Tracker.new([1, 2], 2, 0)
    assert_equal :held, peer.receive(last)
    assert_equal [[Gap.new(1, 1, MAX - 1)], nil], peer.missing(last, 0, 8)
    assert_equal true, peer.absorb(0, [MAX - 1, 0])
    assert_equal [:deliver, last], peer.release
    assert_equal [MAX, 0], peer.clock
  end

  def test_strict_inputs
    assert_raises(ArgumentError) { Tracker.new([1, 1], 1, 0) }
    assert_raises(ArgumentError) { Tracker.new([1, 2], 3, 0) }
    assert_raises(RangeError) { Tracker.new((1..9).to_a, 1, 0) }
    assert_raises(RangeError) { Tracker.restore([1, 2], 1, 0, [0]) }
    assert_raises(TypeError) { Tracker.new(nil, 1, 0) }
    assert_raises(TypeError) { Tracker.allocate }
    node = tracker(1)
    [1.0, Float::NAN, nil, '1', Object.new].each do |bad|
      assert_raises(TypeError) { Tracker.new([1, bad], 1, 0) }
      assert_raises(TypeError) { Tracker.new([1, 2], bad, 0) }
      assert_raises(TypeError) { Tracker.new([1, 2], 1, bad) }
      assert_raises(TypeError) { node.receive(bad) }
      assert_raises(TypeError) { node.check(bad) }
      assert_raises(TypeError) { node.receive(stamp(bad, [1, 0, 0])) }
      assert_raises(TypeError) { node.receive(stamp(2, [1, bad, 0])) }
      assert_raises(TypeError) { node.missing(stamp(2, [1, 1, 0]), bad, 8) }
      assert_raises(TypeError) { node.forget(bad, 1) }
      assert_raises(TypeError) { node.absorb(bad, [0, 0, 0]) }
    end
    assert_raises(TypeError) { node.receive(stamp(2, nil)) }
    assert_raises(RangeError) { node.receive(stamp(2, [0, 1])) }
    [-1, 2**64].each do |n|
      assert_raises(RangeError) { node.receive(stamp(2, [0, n, 0])) }
      assert_raises(RangeError) { node.forget(n, 1) }
    end
    assert_equal [0, 0, 0], node.clock
  end

  def test_freeze_guards
    node = tracker(1)
    incoming = stamp(2, [0, 1, 0])
    node.freeze
    assert_raises(FrozenError) { node.stamp }
    assert_raises(FrozenError) { node.receive(incoming) }
    assert_raises(FrozenError) { node.release }
    assert_raises(FrozenError) { node.forget(2, 1) }
    assert_raises(FrozenError) { node.absorb(0, [0, 0, 0]) }
    assert_equal :deliverable, node.check(incoming)
    assert_equal [[], nil], node.missing(incoming, 0, 8)
    assert_equal [0, 0, 0], node.clock
  end

  def test_gc_stress
    previous = GC.stress
    GC.stress = true
    node = tracker(1)
    sent = node.stamp
    peer = tracker(2)
    assert_equal :held, peer.receive(stamp(1, [2, 0, 0]))
    assert_equal :delivered, peer.receive(sent)
    GC.compact
    assert_equal [:deliver, stamp(1, [2, 0, 0])], peer.release
  ensure
    GC.stress = previous
  end
end
