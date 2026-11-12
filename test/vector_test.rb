# frozen_string_literal: true
require 'minitest/autorun'
require 'causal_machines'

class VectorTest < Minitest::Test
  MAX = 2**64 - 1

  def test_compare_is_a_partial_order
    assert_equal :equal, CausalMachines.compare([1, 2], [1, 2])
    assert_equal :before, CausalMachines.compare([1, 2], [1, 3])
    assert_equal :after, CausalMachines.compare([1, 3], [1, 2])
    assert_equal :concurrent, CausalMachines.compare([2, 1], [1, 2])
    assert_equal :equal, CausalMachines.compare([], [])
  end

  def test_dominance_and_concurrency
    assert CausalMachines.dominates?([1, 3], [1, 2])
    assert CausalMachines.dominates?([1, 2], [1, 2])
    refute CausalMachines.dominates?([1, 2], [1, 3])
    refute CausalMachines.dominates?([2, 1], [1, 2])
    assert CausalMachines.concurrent?([2, 1], [1, 2])
    refute CausalMachines.concurrent?([1, 2], [1, 3])
  end

  def test_merge_is_pointwise_max
    assert_equal [5, 9, MAX], CausalMachines.merge([5, 0, MAX], [2, 9, 3])
    assert_equal [5, 9, MAX], CausalMachines.merge([2, 9, 3], [5, 0, MAX])
    assert_equal [1, 2], CausalMachines.merge([1, 2], [1, 2])
  end

  def test_strict_inputs
    %i[compare dominates? concurrent? merge].each do |name|
      assert_raises(TypeError) { CausalMachines.public_send(name, nil, [1]) }
      assert_raises(TypeError) { CausalMachines.public_send(name, [1], '1') }
      assert_raises(TypeError) { CausalMachines.public_send(name, [1.0], [1]) }
      assert_raises(RangeError) { CausalMachines.public_send(name, [1], [1, 2]) }
      assert_raises(RangeError) { CausalMachines.public_send(name, [-1], [1]) }
      assert_raises(RangeError) { CausalMachines.public_send(name, [2**64], [1]) }
      assert_raises(RangeError) { CausalMachines.public_send(name, [0] * 9, [0] * 9) }
    end
  end

  def test_value_objects
    stamp = CausalMachines::Stamp.new(generation: 1, sender: 2, clock: [0, 1])
    assert_equal({ generation: 1, sender: 2, clock: [0, 1] }, stamp.to_h)
    gap = CausalMachines::Gap.new(3, 1, 4)
    assert_equal({ replica: 3, from: 1, through: 4 }, gap.to_h)
    assert_predicate stamp, :frozen?
    assert_kind_of Data, gap
  end
end
