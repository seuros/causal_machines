# frozen_string_literal: true

require 'minitest/autorun'
require 'open3'
require 'rbconfig'

# The class of the value the extension wraps (Causal) is fixed when it loads.
# Nothing a caller does later, redefining the constant or `Object.const_get`,
# reaches a tracker being built. Each case runs in a fresh process: the class
# is fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'causal_machines'
    C = CausalMachines
    members = C::Membership.new([1, 2, 3])
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: a Causal constant redefined before the first tracker made it an
  # instance of the new class, without any of the gem's methods.
  def test_a_redefined_causal_constant_does_not_change_the_tracker_class
    assert_equal 'true 1 true', isolated(<<~RUBY)
      original = C::Causal
      C.send(:remove_const, :Causal)
      C.const_set(:Causal, Class.new)
      tracker = original.new(members, 1, 0)
      clock = tracker.stamp.clock
      puts [tracker.instance_of?(original), tracker.local, clock == C::VersionVector.new([1, 0, 0])].join(' ')
    RUBY
  end

  # Bug: building the first tracker ran `Object.const_get`, Ruby code in the
  # middle of a native call. No Ruby runs while one is built now.
  def test_building_a_tracker_runs_no_const_get
    assert_equal '0 Causal Causal', isolated(<<~RUBY)
      hits = 0
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        super(*args)
      end
      fresh = C::Causal.new(members, 1, 0)
      restored = C::Causal.restore(members, 2, 0, C::VersionVector.new([1, 0, 0]))
      puts [hits, fresh.class.name.split('::').last, restored.class.name.split('::').last].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal '1 0', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran while building' }
      tracker = C::Causal.new(members, 1, 0)
      puts [tracker.local, tracker.held].join(' ')
    RUBY
  end
end
