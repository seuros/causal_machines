# Changelog

## [0.5.0](https://github.com/seuros/causal_machines/compare/causal-machines-v0.4.0...causal-machines-v0.5.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* validate Page and Gap in the core, refuse unissued missing cursors, seal values, clone-then-keep stamp/release

### Features

* validate Page and Gap in the core, refuse unissued missing cursors, seal values, clone-then-keep stamp/release ([87b24f6](https://github.com/seuros/causal_machines/commit/87b24f6c6c4c295e142fdafa01de7ab5e2dee17b))

## [0.4.0](https://github.com/seuros/causal_machines/compare/causal-machines-v0.3.0...causal-machines-v0.4.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* see CONVENTIONS: one exhaustive Refusal per crate, Result from every mutator, honoured idempotent repeats, Ruby mirrors the core. No compatibility aliases. check and missing can answer Conflict; the Ruby class is Causal; release answers Deliver or Obsolete values.

### Features

* check conflicts on every receive path, Conflict and Full are refusals, forget answers Forgetting ([b2867a8](https://github.com/seuros/causal_machines/commit/b2867a83677cdc2180e9b771ce36eb4ed78624a9))

## [0.3.0](https://github.com/seuros/causal_machines/compare/causal-machines-v0.2.0...causal-machines-v0.3.0) (2026-11-13)


### Features

* implement Default for Gap ([5aa1b2c](https://github.com/seuros/causal_machines/commit/5aa1b2c5029d9d45da2f1f8ca3fdfd19a659b8f7))

## [0.2.0](https://github.com/seuros/causal_machines/compare/causal-machines-v0.1.0...causal-machines-v0.2.0) (2026-11-12)


### Features

* initial extraction ([cc9d39b](https://github.com/seuros/causal_machines/commit/cc9d39b5451f58f4762377dfa5dee280be0cbd9f))
