# frozen_string_literal: true
require 'minitest/autorun'
require 'causal_machines'

# The binding trusts no value it reads, and never loses work when building an
# answer runs Ruby code.
class ValuesTest < Minitest::Test
  include CausalMachines

  def tracker(local) = Causal.new(Membership.new([1, 2, 3]), local, 0)
  def vv(counters) = VersionVector.new(counters)
  def stamp(sender, clock) = Stamp.new(0, sender, vv(clock))

  # Sealing stops accidents, not reflection: a named subclass raises, yet
  # Ruby has already bound its constant, and `Data#initialize` can be bound
  # to skip the validating one. The binding refuses such a value by its
  # exact class before it reaches the core.
  def test_a_leaked_subclass_never_reaches_the_core
    %w[ForgedVector<VersionVector ForgedStamp<Stamp].each do |pair|
      name, parent = pair.split('<')
      assert_raises(TypeError) { eval("class #{name} < CausalMachines::#{parent}; end", binding, __FILE__, __LINE__) }
    end
    assert_equal 'constant', defined?(ForgedVector) # the constant leaks
    [ForgedVector, ForgedStamp].each do |forgery|
      forgery.define_method(:initialize) { |**kw| Data.instance_method(:initialize).bind_call(self, **kw) }
    end
    node = tracker(1)
    forged_clock = ForgedVector.new(counters: [0, 0, 0, 0, 0, 0, 0, 0, 1])
    assert_raises(TypeError) { node.absorb(0, forged_clock) }
    assert_raises(TypeError) { vv([1]).merged(forged_clock) }
    forged_stamp = ForgedStamp.new(generation: 0, sender: 2, clock: vv([0, 1, 0]))
    assert_raises(TypeError) { node.receive(forged_stamp) }
    assert_raises(TypeError) { Deliver.new(forged_stamp) }
    assert_raises(TypeError) { Stamp.new(0, 2, forged_clock) }
    assert_equal [vv([0, 0, 0]), 0], [node.clock, node.held]
  ensure
    %i[ForgedVector ForgedStamp].each do |name|
      self.class.send(:remove_const, name) if self.class.const_defined?(name, false)
    end
  end

  # `Marshal.load` builds an instance of the real class without `initialize`;
  # the binding rebuilds every value it reads through the core.
  # A singleton class opened on a value does not change what it is.
  def test_an_opened_singleton_class_is_still_the_gems_value
    assert_equal :absorbed, tracker(1).absorb(0, vv([0, 1, 0]).tap(&:singleton_class))
  end

  def test_a_marshal_forged_value_never_reaches_the_core
    node = tracker(1)
    nine = forged(VersionVector, [1] * 9)
    assert_raises(RangeError) { node.absorb(0, nine) }
    assert_raises(RangeError) { node.receive(forged(Stamp, 0, 2, nine)) }
    assert_raises(RangeError) { Causal.new(forged(Membership, [1, 1]), 1, 0) }
    assert_raises(RangeError) { Page.new([forged(Gap, 1, 3, 2)], nil) }
    assert_raises(RangeError) { Obsolete.new(forged(Stamp, 0, 2, nine)) }
    assert_equal [vv([0, 0, 0]), 0], [node.clock, node.held]
  end

  def test_stamp_applies_nothing_when_building_its_answer_raises_re_enters_or_freezes
    [
      [IOError, ->(_) { raise IOError }],
      [RuntimeError, ->(node) { node.absorb(0, vv([0, 0, 4])) }],
      [FrozenError, lambda(&:freeze)]
    ].each do |error, hook|
      node = tracker(1)
      replacing(Stamp, ->(*) { hook.call(node) }) { assert_raises(error) { node.stamp } }
      expected = error == RuntimeError ? vv([0, 0, 4]) : vv([0, 0, 0])
      assert_equal expected, node.clock
    end
  end

  def test_release_applies_nothing_when_building_its_answer_raises_re_enters_or_freezes
    [
      [IOError, ->(_) { raise IOError }],
      [RuntimeError, ->(node) { node.forget(1, 2) }],
      [FrozenError, lambda(&:freeze)]
    ].each do |error, hook|
      node = tracker(3)
      assert_equal :held, node.receive(stamp(1, [2, 0, 0]))
      assert_equal :absorbed, node.absorb(0, vv([2, 0, 0]))
      replacing(Obsolete, ->(*) { hook.call(node) }) { assert_raises(error) { node.release } }
      assert_equal(error == RuntimeError ? 0 : 1, node.held)
    end
    node = tracker(3)
    node.receive(stamp(1, [2, 0, 0]))
    node.absorb(0, vv([2, 0, 0]))
    assert_equal Obsolete.new(stamp(1, [2, 0, 0])), node.release
  end

  # No borrow is held while a reader builds its answer, so a re-entrant call
  # from `new` works instead of aborting the process.
  def test_readers_hold_no_borrow_while_building
    node = tracker(1)
    replacing(VersionVector, ->(*) { node.absorb(0, vv([0, 1, 0])) }) { node.clock }
    replacing(Membership, ->(*) { node.absorb(0, vv([0, 2, 0])) }) { node.membership }
    replacing(Page, ->(*) { node.absorb(0, vv([0, 3, 0])) }) { node.missing(stamp(2, [0, 5, 0]), 0, 1) }
    assert_equal vv([0, 3, 0]), node.clock
  end

  # The classes are resolved once and pinned: after a compaction that moves
  # everything movable, every path that checks or builds a value still finds
  # them (unpinned, the exact-class check would refuse the gem's own values).
  def test_classes_survive_gc_compaction
    if GC.respond_to?(:verify_compaction_references)
      GC.verify_compaction_references(expand_heap: true, toward: :empty)
    elsif GC.respond_to?(:compact)
      GC.compact
    end
    node = tracker(1)
    assert_instance_of Stamp, node.stamp
    assert_instance_of VersionVector, node.clock
    assert_instance_of Membership, node.membership
    assert_instance_of VersionVector, vv([1]).increment(1)
    assert_instance_of VersionVector, vv([1]).merged(vv([0, 1]))
    page = node.missing(stamp(2, [0, 3, 0]), 0, 8)
    assert_instance_of Page, page
    assert_instance_of Gap, page.gaps.first
    peer = tracker(3)
    peer.receive(stamp(2, [0, 2, 0]))
    peer.receive(stamp(2, [0, 1, 0]))
    assert_instance_of Deliver, peer.release
    peer.receive(stamp(1, [2, 0, 0]))
    peer.absorb(0, vv([2, 0, 0]))
    assert_instance_of Obsolete, peer.release
  end

  private

  # Runs the block with `klass.new` calling `hook` first, once; the original
  # `new` is restored afterwards.
  def replacing(klass, hook)
    original = klass.method(:new)
    armed = true
    klass.define_singleton_method(:new) do |*args, **kw|
      if armed
        armed = false
        hook.call(*args)
      end
      original.call(*args, **kw)
    end
    yield
  ensure
    klass.singleton_class.send(:remove_method, :new)
    klass.define_singleton_method(:new, original)
  end

  # An instance of `klass` holding `fields`, loaded by Marshal from a look-alike
  # so its `initialize` never ran.
  def forged(klass, *fields)
    stand_in = Data.define(*klass.members)
    Object.const_set(:MarshalStandIn, stand_in)
    bytes = Marshal.dump(stand_in.new(*fields))
    name = klass.name
    Marshal.load(bytes.sub(":#{(5 + 14).chr}MarshalStandIn".b, ":#{(5 + name.size).chr}#{name}".b))
  ensure
    Object.send(:remove_const, :MarshalStandIn)
  end
end
