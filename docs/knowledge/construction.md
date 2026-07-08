# Setup and construction

When an operation's entry point is a method, or needs a constructed object as
a parameter, a `#[spec_setup("operation")]` function builds it. A setup is
*arrange*, not *act*: **it never appears in the trace as an operation** (no
`$run`, no setup-named events), and it is never named in the spec. The spec sees
only `inputs -> operation -> outputs`; the harness routes each input by name to
either the operation call or a setup constructor.

## `spec_setup` rules

- **No `self` / `this`** — setups are free functions or static factories.
- **First argument is the operation name** — `#[spec_setup("increment")]`
  links the setup to the operation it prepares (not the setup's own name).
- **Matched by type** — the setup's return value fills the operation's method
  receiver or a parameter whose type matches the return type.
- **Construction inputs are ordinary operation inputs** — a setup's parameters
  are declared in the operation's `inputs:` and supplied in the case's
  `inputs:`, exactly like the operation's own arguments. The harness routes each
  value to the setup or the call by name; the spec draws no distinction.
- **`fills` disambiguates** — when an operation has more than one parameter of
  the setup's output type, or more than one setup produces that type, each
  setup pins its target with `fills = "<param>"`. Multiple `#[spec_setup]`
  attributes may be stacked on one function to fill several params.
- **Side-effect setups** — a setup whose return value matches nothing is
  simply invoked before the operation.

```rust
// stateless: no setup needed
#[spec_operation("add")]
fn add(a: i32, b: i32) -> i32 { a + b }

// stateful: setup constructs the receiver, linked to the operation
#[spec_setup("increment")]
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
  operation: increment
  expected:
    - count: "0"
    - $run: increment
    - count: "1"
```

The `count` events are the receiver's captured fields (a `#[spec_event]` on
`Counter.count`), not the setup — the setup itself emits nothing.

## Setup with a construction input

If a setup takes parameters, declare them as ordinary operation inputs and
supply them in the case's `inputs:`; the harness routes them to the setup by
name (no separate `setup:` key):

```rust
#[spec_setup("increment")]
fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}
```

```yaml
operations:
  increment:
    inputs: { initial: i32 }  # routed to make_counter by name
    outputs: [count]
cases:
  - name: start_at_10
    operation: increment
    inputs: { initial: 10 }
    expected:
      - count: "10"
      - $run: increment
      - count: "11"
```

See `test/rust/crates/specgate-fixtures/specs/setup_with_params.spec.yaml`.

Because construction inputs are ordinary operation inputs, the existing
`input_completeness` check validates them for free: a typo (e.g. `intial`) is
reported as an extra input plus a missing required input, so a mistyped input
can't silently construct with a defaulted value.

## Multiple constructed objects of the same type

When an operation takes more than one object of the same type, each setup
pins itself to a parameter with `fills`. The parameter role becomes the
prefix for `#[spec_event]` trace names:

```rust
#[spec_setup("transfer", fills = "source")]
fn make_source() -> Account { Account { balance: 100 } }

#[spec_setup("transfer", fills = "target")]
fn make_target() -> Account { Account { balance: 0 } }

struct Account { #[spec_event] balance: i32 }

#[spec_operation("transfer")]
fn transfer(source: &mut Account, target: &mut Account, amount: i32) {
    source.balance -= amount;
    target.balance += amount;
}
```

```yaml
- name: transfer_between_accounts
  operation: transfer
  inputs: { amount: 50 }        # the operation's own input
  expected:
    - source.balance: "100"
    - target.balance: "0"
    - $run: transfer
    - source.balance: "50"
    - target.balance: "50"
```

Here the setups are parameterless, so only `amount` (the operation's own input)
appears. See `multi_setup.spec.yaml`.

One setup may also fill several same-typed params by stacking
`#[spec_setup(..., fills = ...)]` — see `shared_setup.spec.yaml`. When such a
setup needs distinct construction inputs per fill, declare each as a flat
`<param>_<fills>` operation input (e.g. `start_left`, `start_right`); a
single-receiver setup routes flat by the setup's bare parameter name.

```yaml
operations:
  combine:
    inputs: { start_left: i32, start_right: i32 }
    outputs: [left.value, right.value, $result: i32]
cases:
  - name: one_setup_fills_two_params
    operation: combine
    inputs: { start_left: 10, start_right: 5 }
    expected:
      - left.value: "10"
      - right.value: "5"
      - $run: combine
      - $result: "15"
```
