# Setup and construction

When an operation's entry point is a method, or needs a constructed object as
a parameter, a `#[spec_setup("operation")]` function builds it. **Setups are
invisible to the spec** — the spec never names a setup, declares `kind: setup`,
or carries a `setup:` field. The link lives entirely in code.

## `spec_setup` rules

- **No `self` / `this`** — setups are free functions or static factories.
- **First argument is the operation name** — `#[spec_setup("increment")]`
  links the setup to the operation it prepares (not the setup's own name).
- **Matched by type** — the setup's return value fills the operation's method
  receiver or a parameter whose type matches the return type.
- **Construction inputs are operation inputs** — a setup's parameters are
  declared on the operation and routed to the setup by name. They do **not**
  emit setup-specific events (setups are invisible).
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

## Setup with a construction input

If a setup takes parameters, declare them as operation inputs; they route to
the setup by name (no setup event is emitted):

```rust
#[spec_setup("increment")]
fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}
```

```yaml
operations:
  increment:
    inputs: { initial: i32 }
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
  inputs: { amount: 50 }
  expected:
    - source.balance: "100"
    - target.balance: "0"
    - $run: transfer
    - source.balance: "50"
    - target.balance: "50"
```

See `multi_setup.spec.yaml`. One setup may also fill several same-typed params
by stacking `#[spec_setup(..., fills = ...)]` — see `shared_setup.spec.yaml`.
When such a setup needs distinct construction inputs per fill, give each as a
flat input named `<param>_<fills>` (e.g. `start_left`, `start_right`); the
construction value routes flat by the setup's parameter name otherwise.
