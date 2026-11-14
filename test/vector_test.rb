# frozen_string_literal: true

require 'minitest/autorun'
require 'causal_machines'

# VersionVector, Membership and the plain values validate through the core.
class VectorTest < Minitest::Test
  include CausalMachines

  MAX = 2**64 - 1

  def vv(*counters) = VersionVector.new(counters)

  def test_vectors_hold_every_slot
    assert_equal [0] * 8, VersionVector.new.counters
    assert_equal [1, 2, 0, 0, 0, 0, 0, 0], vv(1, 2).counters
    assert_equal vv(1, 2), VersionVector.new([1, 2, 0, 0])
    assert_predicate vv(1).counters, :frozen?
    assert_equal 2, vv(1, 2).get(1)
    assert_equal 0, vv(1, 2).get(7)
    assert_nil vv(1, 2).get(8)
    assert_equal({ counters: [3] + [0] * 7 }, vv(3).to_h)
  end

  def test_increment_answers_a_new_vector_or_a_refusal
    start = vv(1, MAX)
    assert_equal vv(2, MAX), start.increment(0)
    assert_equal vv(1, MAX), start
    assert_equal :exhausted, start.increment(1)
    assert_equal :unknown_member, start.increment(8)
  end

  def test_compare_is_a_partial_order
    assert_equal :equal, vv(1, 2).compare(vv(1, 2))
    assert_equal :before, vv(1, 2).compare(vv(1, 3))
    assert_equal :after, vv(1, 3).compare(vv(1, 2))
    assert_equal :concurrent, vv(2, 1).compare(vv(1, 2))
    assert_equal :equal, VersionVector.new.compare(vv)
  end

  def test_dominance_and_concurrency
    assert vv(1, 3).dominates?(vv(1, 2))
    assert vv(1, 2).dominates?(vv(1, 2))
    refute vv(1, 2).dominates?(vv(1, 3))
    refute vv(2, 1).dominates?(vv(1, 2))
    assert vv(2, 1).concurrent?(vv(1, 2))
    refute vv(1, 2).concurrent?(vv(1, 3))
  end

  def test_merged_is_pointwise_max
    assert_equal vv(5, 9, MAX), vv(5, 0, MAX).merged(vv(2, 9, 3))
    assert_equal vv(5, 9, MAX), vv(2, 9, 3).merged(vv(5, 0, MAX))
    assert_equal vv(1, 2), vv(1, 2).merged(vv(1, 2))
  end

  def test_vector_strict_inputs
    [nil, '1', 1].each { |bad| assert_raises(TypeError) { VersionVector.new(bad) } }
    [1.0, nil, '1'].each { |bad| assert_raises(TypeError) { vv(1, bad) } }
    [-1, 2**64].each { |bad| assert_raises(RangeError) { vv(bad) } }
    assert_raises(RangeError) { VersionVector.new([0] * 9) }
    %i[compare dominates? concurrent? merged].each do |name|
      assert_raises(TypeError) { vv(1).public_send(name, [1]) }
      assert_raises(TypeError) { vv(1).public_send(name, nil) }
    end
    assert_raises(TypeError) { vv(1).get(nil) }
    assert_raises(TypeError) { vv(1).increment(1.0) }
    assert_raises(RangeError) { vv(1).get(-1) }
  end

  def test_membership_binds_slots
    members = Membership.new([10, 20, 30])
    assert_equal [10, 20, 30], members.ids
    assert_predicate members.ids, :frozen?
    assert_equal 3, members.len
    refute_predicate members, :empty?
    assert_predicate Membership.new([]), :empty?
    assert_equal 1, members.slot(20)
    assert_nil members.slot(40)
    assert_equal 30, members.replica(2)
    assert_nil members.replica(3)
    assert members.admits?(vv(1, 2, 3))
    refute members.admits?(vv(0, 0, 0, 1))
  end

  def test_membership_strict_inputs
    assert_raises(RangeError) { Membership.new([1, 1]) }
    assert_raises(RangeError) { Membership.new((1..9).to_a) }
    assert_raises(RangeError) { Membership.new([-1]) }
    [nil, 1, [1.0], ['1']].each { |bad| assert_raises(TypeError) { Membership.new(bad) } }
    assert_raises(TypeError) { Membership.new([1]).slot(nil) }
    assert_raises(TypeError) { Membership.new([1]).admits?([1]) }
  end

  def test_stamp_release_gap_and_page_validate
    stamp = Stamp.new(generation: 1, sender: 2, clock: vv(0, 1))
    assert_equal({ generation: 1, sender: 2, clock: vv(0, 1) }, stamp.to_h)
    assert_predicate stamp, :frozen?
    assert_raises(TypeError) { Stamp.new(1, 2, [0, 1]) }
    assert_raises(TypeError) { Stamp.new(1.0, 2, vv) }
    assert_raises(TypeError) { Stamp.new(1, nil, vv) }
    assert_raises(RangeError) { Stamp.new(-1, 2, vv) }
    assert_equal stamp, Deliver.new(stamp).stamp
    assert_equal stamp, Obsolete.new(stamp).stamp
    [nil, vv, Deliver.new(stamp)].each do |bad|
      assert_raises(TypeError) { Deliver.new(bad) }
      assert_raises(TypeError) { Obsolete.new(bad) }
    end
    assert_equal({ replica: 3, from: 1, through: 4 }, Gap.new(3, 1, 4).to_h)
    assert_raises(RangeError) { Gap.new(3, 0, 4) }
    assert_raises(RangeError) { Gap.new(3, 5, 4) }
    assert_raises(TypeError) { Gap.new(3, 1.0, 4) }
    gap = Gap.new(1, 1, 1)
    assert_equal [gap], Page.new([gap], 7).gaps
    assert_predicate Page.new([gap], nil).gaps, :frozen?
    assert_raises(RangeError) { Page.new([gap], 8) }
    assert_raises(RangeError) { Page.new([gap] * 9, nil) }
    # A page that stops early stops after the slots it wrote: no more gaps
    # than slots before the cursor (the core's `Page::new`).
    assert_equal 2, Page.new([gap, gap], 2).gaps.size
    assert_raises(RangeError) { Page.new([gap, gap], 1) }
    assert_raises(TypeError) { Page.new([[1, 1, 1]], nil) }
    assert_raises(TypeError) { Page.new(nil, nil) }
    assert_raises(TypeError) { Page.new([], 1.0) }
  end

  # A subclass overriding `initialize` without `super`, or `allocate`, would
  # build a value that never ran the core's check.
  def test_values_are_final
    [VersionVector, Membership, Stamp, Deliver, Obsolete, Gap, Page].each do |value|
      assert_raises(TypeError) { Class.new(value) { def initialize(**) = nil } }
      assert_raises(NoMethodError) { value.allocate }
    end
    assert_raises(RangeError) { Gap.new(1, 1, 2).with(from: 0) }
  end
end
