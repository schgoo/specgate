# Construction resolution and setup

When an operation's entry point is a method (takes `self`/`this`), the harness
needs to construct the owning type. This is where `spec_setup` comes in.

## Resolution algorithm

The harness works backwards from the entry point:

1. **Entry point** — what types does it need? (self type, parameter types)
2. **For each type, try in order:**
   - Auto-discover a single public constructor → use it, recurse on its params
   - Find a `spec_setup` that returns this type → use it
   - Primitive/leaf type → provided by test case inputs
3. **If unresolvable** — validation error with actionable suggestion

All resolved constructor/setup parameters bubble up as flat test case inputs.

## spec_setup

Setup functions are test fixtures. They live in test code, not production code.

```rust
#[spec_setup("find_user", name = "default")]
fn setup_request(tenant: String, token: String) -> Request {
    let mut r = Request::new(tenant, token);
    r.set_endpoint("/users");
    r
}
```

### Rules

- **No `self`** — setups are free functions or associated functions, never methods
- **`name` is required** — it's the language-agnostic join key referenced from spec cases
- **Names must be unique** per operation within a crate/project
- **Return type matters** — the harness matches setups to types by return type
- **Multiple setups allowed** — different setups can return different types for the same operation

### Environment setup

A setup that returns `()` configures the environment rather than constructing a type:

```rust
#[spec_setup("find_user", name = "runtime")]
fn init_runtime(mode: String) {
    std::env::set_var("RUNTIME_MODE", mode);
}
```

### When no setup is needed

- Entry point is a free function → no construction needed
- Entry point takes only primitive parameters → provided directly by test case

### Ambiguous construction warning

If the entry point is a method but no `spec_setup` exists, `core.validate`
emits an `AmbiguousConstruction` warning. The implementation may still work
(auto-discovered constructor), but explicit setup is preferred for clarity.

## Full example: StateMachine with setup

```rust
struct CircuitBreaker {
    #[spec_capture("breaker")]
    state: String,
    #[spec_capture("breaker")]
    failure_count: u32,
}

impl CircuitBreaker {
    fn new(threshold: u32) -> Self { ... }

    #[spec_operation("breaker", kind = StateMachine)]
    fn on_result(&mut self, success: bool) { ... }

    #[spec_mock("breaker", name = "backend")]
    fn call_backend(&self) -> bool { ... }
}

#[spec_setup("breaker", name = "default")]
fn setup_breaker(threshold: u32) -> CircuitBreaker {
    CircuitBreaker::new(threshold)
}
```

The harness resolves: `on_result` needs `CircuitBreaker` → found `setup_breaker`
→ `setup_breaker` needs `threshold: u32` (primitive) → `threshold` becomes a
test case input.
