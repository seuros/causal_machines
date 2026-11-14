# frozen_string_literal: true
require 'minitest/autorun'
require 'causal_machines'

class CausalTest < Minitest::Test
  include CausalMachines

  MAX = 2**64 - 1

  def tracker(local, members: [1, 2, 3], generation: 0)
    Causal.new(Membership.new(members), local, generation)
  end

  def vv(counters) = VersionVector.new(counters)

  def stamp(sender, clock, generation: 0)
    Stamp.new(generation, sender, vv(clock))
  end

  def test_configuration_and_constants
    node = tracker(2, generation: 9)
    assert_equal 8, MEMBERS
    assert_equal 64, CAPACITY
    assert_equal 64, node.capacity
    assert_equal Membership.new([1, 2, 3]), node.membership
    assert_equal 2, node.local
    assert_equal 9, node.generation
    assert_equal vv([0, 0, 0]), node.clock
    assert_equal 0, node.held
  end

  def test_causal_order_survives_reordering
    alice = tracker(1)
    bob = tracker(2)
    carol = tracker(3)
    question = alice.stamp
    assert_equal stamp(1, [1, 0, 0]), question
    assert_equal :delivered, bob.receive(question)
    answer = bob.stamp
    assert_equal vv([1, 1, 0]), answer.clock

    assert_equal :missing, carol.check(answer)
    assert_equal :held, carol.receive(answer)
    assert_equal :already_held, carol.receive(answer)
    assert_equal 1, carol.held
    assert_nil carol.release
    assert_equal Page.new([Gap.new(1, 1, 1)], nil), carol.missing(answer, 0, 8)
    assert_equal :deliverable, carol.check(question)
    assert_equal :delivered, carol.receive(question)
    assert_equal CausalMachines::Deliver.new(answer), carol.release
    assert_nil carol.release
    assert_equal vv([1, 1, 0]), carol.clock
    assert_equal :duplicate, carol.receive(question)
    assert_equal :duplicate, carol.check(answer)
    assert_equal :duplicate, alice.receive(question)
  end

  def test_missing_pages_resume_from_the_cursor
    node = tracker(3)
    wanted = stamp(2, [3, 4, 2])
    first = node.missing(wanted, 0, 2)
    assert_instance_of Page, first
    assert_equal %i[gaps next], Page.members
    assert_equal [[:stamp], [:stamp]], [CausalMachines::Deliver.members, CausalMachines::Obsolete.members]
    assert_equal [Gap.new(1, 1, 3), Gap.new(2, 1, 3)], first.gaps
    assert_equal 2, first.next
    assert_equal Page.new([Gap.new(3, 1, 2)], nil), node.missing(wanted, first.next, 2)
    assert_equal Page.new([], 0), node.missing(wanted, 0, 0)
    assert_equal Page.new([], nil), node.missing(stamp(1, [1, 0, 0]), 0, 8)
    assert_raises(RangeError) { node.missing(wanted, 0, 9) }
    # A cursor past the last member slot was never handed out: refused,
    # whatever the stamp's readiness, once the stamp itself is judged.
    [3, 7, 2**64 - 1].each do |cursor|
      assert_equal :unknown_member, node.missing(wanted, cursor, 2)
      assert_equal :unknown_member, node.missing(stamp(1, [1, 0, 0]), cursor, 2)
    end
    assert_equal :future_generation, node.missing(stamp(2, [3, 4, 2], generation: 1), 3, 2)
    assert_raises(RangeError) { node.missing(wanted, 2**64, 2) }
  end

  def test_full_conflict_and_forget
    node = tracker(3)
    (2..65).each { |n| assert_equal :held, node.receive(stamp(1, [n, 0, 0])) }
    assert_equal 64, node.held
    assert_equal :full, node.receive(stamp(1, [66, 0, 0]))
    assert_equal :conflict, node.receive(stamp(1, [2, 1, 0]))
    assert_equal :already_held, node.receive(stamp(1, [2, 0, 0]))
    assert_equal 64, node.held
    assert_equal vv([0, 0, 0]), node.clock
    assert_equal :unknown_member, node.forget(99, 1)
    assert_equal :not_held, node.forget(2, 65)
    assert_equal 64, node.held
    assert_equal :forgotten, node.forget(1, 65)
    assert_equal 63, node.held
    assert_equal :not_held, node.forget(1, 65)
    assert_equal 63, node.held
    assert_equal :held, node.receive(stamp(1, [66, 0, 0]))
    assert_equal :delivered, node.receive(stamp(1, [1, 0, 0]))
    released = []
    while (out = node.release)
      released << out
    end
    assert_equal((2..64).map { |n| CausalMachines::Deliver.new(stamp(1, [n, 0, 0])) }, released)
    assert_equal 1, node.held
  end

  def test_absorb_and_obsolete_release
    node = tracker(3)
    late = stamp(1, [2, 0, 0])
    assert_equal :held, node.receive(late)
    assert_equal :absorbed, node.absorb(0, vv([4, 0, 0]))
    assert_equal vv([4, 0, 0]), node.clock
    assert_equal CausalMachines::Obsolete.new(late), node.release
    assert_nil node.release
    assert_equal :stale_generation, tracker(3, generation: 2).absorb(1, vv([1, 0, 0]))
    assert_equal :future_generation, node.absorb(1, vv([1, 0, 0]))
    assert_equal :malformed, node.absorb(0, vv([5, 0, 0, 1]))
    assert_equal vv([4, 0, 0]), node.clock
  end

  def test_absorbing_a_covered_clock_is_an_honoured_repeat
    node = tracker(3)
    assert_equal :absorbed, node.absorb(0, vv([4, 1, 0]))
    [[4, 1, 0], [2, 0, 0], [0, 0, 0], []].each do |covered|
      assert_equal :already_covered, node.absorb(0, vv(covered))
      assert_equal vv([4, 1, 0]), node.clock
    end
    assert_equal :absorbed, node.absorb(0, vv([3, 2, 0]))
    assert_equal vv([4, 2, 0]), node.clock
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
    assert_equal vv([0, 0, 0]), node.clock
    assert_equal 0, node.held
  end

  def test_no_mutating_call_answers_a_boolean
    node = tracker(3)
    late = stamp(1, [2, 0, 0])
    answers = [
      node.stamp, node.receive(late), node.receive(late), node.forget(1, 2), node.forget(1, 2),
      node.forget(9, 2), node.absorb(0, vv([1, 0, 0])), node.absorb(1, vv([1, 0, 0])), node.release
    ]
    assert_empty(answers.select { [true, false].include?(_1) })
    assert_equal %i[held already_held forgotten not_held unknown_member absorbed future_generation],
                 answers.grep(Symbol)
    assert_equal :already_covered, node.absorb(0, vv([1, 0, 0]))
  end

  def test_restore_and_u64_limits
    node = Causal.restore(Membership.new([1, 2]), 1, 0, vv([MAX - 1, 0]))
    last = node.stamp
    assert_equal vv([MAX, 0]), last.clock
    assert_equal :exhausted, node.stamp
    assert_equal vv([MAX, 0]), node.clock
    peer = Causal.new(Membership.new([1, 2]), 2, 0)
    assert_equal :held, peer.receive(last)
    assert_equal Page.new([Gap.new(1, 1, MAX - 1)], nil), peer.missing(last, 0, 8)
    assert_equal :absorbed, peer.absorb(0, vv([MAX - 1, 0]))
    assert_equal CausalMachines::Deliver.new(last), peer.release
    assert_equal vv([MAX, 0]), peer.clock
  end

  def test_strict_inputs
    pair = Membership.new([1, 2])
    assert_raises(RangeError) { Causal.new(pair, 3, 0) }
    assert_raises(RangeError) { Causal.restore(pair, 1, 0, vv([0, 0, 1])) }
    [nil, [1, 2], Object.new].each do |bad|
      assert_raises(TypeError) { Causal.new(bad, 1, 0) }
      assert_raises(TypeError) { Causal.restore(pair, 1, 0, bad) }
    end
    assert_raises(TypeError) { Causal.allocate }
    node = tracker(1)
    [1.0, Float::NAN, nil, '1', Object.new].each do |bad|
      assert_raises(TypeError) { Causal.new(pair, bad, 0) }
      assert_raises(TypeError) { Causal.new(pair, 1, bad) }
      assert_raises(TypeError) { node.receive(bad) }
      assert_raises(TypeError) { node.check(bad) }
      assert_raises(TypeError) { node.missing(stamp(2, [1, 1, 0]), bad, 8) }
      assert_raises(TypeError) { node.forget(bad, 1) }
      assert_raises(TypeError) { node.absorb(bad, vv([0, 0, 0])) }
      assert_raises(TypeError) { node.absorb(0, bad) }
    end
    assert_equal :malformed, node.receive(stamp(2, [0, 1, 0, 1]))
    [-1, 2**64].each { |n| assert_raises(RangeError) { node.forget(n, 1) } }
    assert_equal vv([0, 0, 0]), node.clock
  end

  def test_freeze_guards
    node = tracker(1)
    incoming = stamp(2, [0, 1, 0])
    node.freeze
    assert_raises(FrozenError) { node.stamp }
    assert_raises(FrozenError) { node.receive(incoming) }
    assert_raises(FrozenError) { node.release }
    assert_raises(FrozenError) { node.forget(2, 1) }
    assert_raises(FrozenError) { node.absorb(0, vv([0, 0, 0])) }
    assert_equal :deliverable, node.check(incoming)
    assert_equal Page.new([], nil), node.missing(incoming, 0, 8)
    assert_equal vv([0, 0, 0]), node.clock
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
    assert_equal CausalMachines::Deliver.new(stamp(1, [2, 0, 0])), peer.release
  ensure
    GC.stress = previous
  end
end
