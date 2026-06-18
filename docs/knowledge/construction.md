# Setup and construction

When an operation's entry point is a method or otherwise needs an
existing object, the case names a `#[spec_setup("…")]` function that
constructs it.

## `spec_setup` rules

- **No `self` / `this`** — setups are free functions or static factories.
- **Single name argument** — `#[spec_setup("make_counter")]`. The name
  is the join key that cases reference via `setup:`.
- **Parameters become inputs** — each parameter is bound from the case's
  `inputs:` map by name, and is emitted as
  `Event { name: "<setup>.<param>", value }`.
- **Multiple setups allowed** — a single source file can declare any
  number of setups; cases pick the one they want by name.

```rust
// stateless: no setup needed
#[spec_operation("add")]
fn add(a: i32, b: i32) -> i32 { a + b }

// stateful: setup constructs the receiver
#[spec_setup("make_counter")]
fn make_counter() -> Counter {
    Counter { count: 0 }
}

struct Counter {
    #[spec_event]
    count: i32,
}

impl Counter {
    #[spec_operation("increment")]
    fn increment(&mut self) { self.count += 1; }
}
```

```yaml
- name: increment_once
  setup: make_counter
  operation: increment
  expected:
    - count: "0"
    - run: increment
    - count: "1"
```

## Setup with parameters

If a setup takes parameters, pass them through the case `inputs:` map:

```rust
#[spec_setup("make_counter")]
fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}
```

```yaml
- name: start_at_10
  setup: make_counter
  operation: increment
  inputs: { initial: 10 }
  expected:
    - make_counter.initial: "10"   # setup parameter event
    - count: "10"
    - run: increment
    - count: "11"
```

See `test/rust/crates/specgate-fixtures/specs/setup_with_params.spec.yaml`.

## Multiple setups in one case

When an operation takes more than one constructed object, the case maps
**aliases** to setup function names. The aliases become both the
operation parameter names and the prefix for `#[spec_event]` trace
names:

```rust
#[spec_setup("make_source")] fn make_source() -> Account { Account { balance: 100 } }
#[spec_setup("make_target")] fn make_target() -> Account { Account { balance: 0 } }

struct Account { #[spec_event] balance: i32 }

#[spec_operation("transfer")]
fn transfer(source: &mut Account, target: &mut Account, amount: i32) {
    source.balance -= amount;
    target.balance += amount;
}
```

```yaml
- name: transfer_between_accounts
  setup:
    source: make_source
    target: make_target
  operation: transfer
  inputs: { amount: 50 }
  expected:
    - source.balance: "100"
    - target.balance: "0"
    - run: transfer
    - source.balance: "50"
    - target.balance: "50"
```

See `multi_setup.spec.yaml`.
