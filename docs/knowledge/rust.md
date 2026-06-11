# Rust implementation conventions

## Project structure

```
crates/<component-name>/
  Cargo.toml
  src/
    lib.rs          # implementation
  tests/
    spec_tests.rs   # generated from spec cases
```

Add the crate to the workspace `Cargo.toml` if one exists.

## Common dependencies

- `serde` + `serde_json` + `serde_yaml` — serialization
- `thiserror` — error type derivation
- `clap` — CLI argument parsing (if building a CLI)

## Annotations (when available)

Rust uses proc macros from the SpecGate annotation crate.

| Macro | Form | Example |
|-------|------|---------|
| `#[spec_operation("name", kind = K)]` | attribute | `#[spec_operation("calc", kind = StateMachine)]` |
| `#[spec_setup("op", name = "n")]` | attribute | `#[spec_setup("calc", name = "default")]` |
| `#[spec_checkpoint("op")]` | attribute | `#[spec_checkpoint("pipeline")]` on a method |
| `spec_checkpoint!("op", expr)` | inline | `spec_checkpoint!("pipeline", self.store.count(item))` |
| `#[spec_capture("op")]` | attribute | `#[spec_capture("calc")]` on struct or field |
| `#[spec_mock("op", name = "n")]` | attribute | `#[spec_mock("calc", name = "backend")]` |

Symbol paths are fully qualified: `my_crate::module::Type::method`.
Use `pub(crate)` or `#[cfg(test)]` for test access.

```rust
// Struct-level capture — all public fields captured
#[spec_capture("fetch")]
struct FetchResult {
    status_code: u16,
    body: String,
}

impl FetchResult {
    #[spec_operation("fetch", kind = Stateless)]
    pub fn fetch(url: &str) -> FetchResult { /* ... */ todo!() }
}

#[spec_setup("fetch", name = "default")]
fn setup_fetch(url: String) -> FetchResult {
    FetchResult::fetch(&url)
}
```

```rust
// Field-level capture — only annotated fields captured
struct CircuitBreaker {
    #[spec_capture("breaker")]
    state: String,
    #[spec_capture("breaker")]
    failure_count: u32,
    internal_timer: u64,  // not captured
}

impl CircuitBreaker {
    #[spec_operation("breaker", kind = StateMachine)]
    pub fn on_result(&mut self, success: bool) { /* ... */ }

    #[spec_mock("breaker", name = "backend")]
    fn call_backend(&self) -> bool { /* ... */ true }
}

#[spec_setup("breaker", name = "default")]
fn setup_breaker(threshold: u32) -> CircuitBreaker {
    CircuitBreaker { state: "closed".into(), failure_count: 0, internal_timer: 0 }
}
```

```rust
// Checkpoint — attribute form (every call recorded)
impl Pipeline {
    #[spec_checkpoint("pipeline")]
    fn validate(&self, input: &str) -> bool { /* ... */ true }
}

// Checkpoint — inline form (third-party types, specific expressions)
#[spec_operation("pipeline", kind = Sequence)]
fn process(&self, input: &str) -> Output {
    let valid = spec_checkpoint!("pipeline", self.validator.check(input));
    let parsed = spec_checkpoint!("pipeline", third_party::parse(input));
    Output { valid, parsed }
}
```

### Zero-cost in production

Add the annotation crate with the `specgate` feature:

```toml
[dependencies]
specgate-macros = { version = "0.1", features = ["specgate"] }
```

- **Feature enabled (`specgate`)**: annotations are active, values are captured.
- **Feature disabled (default)**: all macros expand to no-ops. Attribute macros
  vanish; `spec_checkpoint!(expr)` evaluates to just `expr`.
- **Release builds**: disable the feature by default. Enable explicitly for
  integration testing in staging if needed.

## Mapping spec types to Rust

| Spec type | Rust type |
|-----------|-----------|
| `string` | `String` |
| `int` | `i64` |
| `float` | `f64` |
| `bool` | `bool` |
| `List<T>` | `Vec<T>` |
| `Option<T>` | `Option<T>` |
| `Map<K, V>` | `HashMap<K, V>` or `BTreeMap<K, V>` |
| `oneof` | `enum` with variants |
| record (fields) | `struct` with named fields |

## Mapping spec oneof to Rust enum

```yaml
# Spec
types:
  Shape:
    oneof:
      Circle: { radius: float }
      Rectangle: { width: float, height: float }
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Shape {
    Circle { radius: f64 },
    Rectangle { width: f64, height: f64 },
}
```

## Generating tests from spec cases

Each spec case maps to one `#[test]` function:

```rust
#[test]
fn circle_area() {
    // Arrange
    let shape = Shape::Circle { radius: 5.0 };

    // Act
    let area = compute_area(&shape);

    // Assert
    assert!((area - 78.54).abs() < 0.01);
}
```

For oneof outcomes, match on variants:

```rust
#[test]
fn divide_by_zero() {
    let result = divide(10, 0);
    match result {
        CalcResult::Error { message } => {
            assert_eq!(message, "division by zero");
        }
        other => panic!("expected Error, got {:?}", other),
    }
}
```

## Error handling

Use `thiserror` for domain error types:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("unexpected token: {token}")]
    UnexpectedToken { token: String },
    #[error("missing closing delimiter")]
    MissingDelimiter,
}
```
