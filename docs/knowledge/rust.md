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
- `ohno` — error types (project convention — never use thiserror/anyhow)
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

Use `ohno` (from [microsoft/oxidizer](https://github.com/microsoft/oxidizer))
for all error types. Never use thiserror/anyhow.

ohno uses **structs, not enums**. Enum variants are publicly accessible and
become API surface — adding/removing variants is a breaking change. Struct-based
errors avoid this, and each error becomes a first-class type with its own
metadata, backtrace, and enrichment chain. `#[ohno::error]` adds an `OhnoCore`
field and derives `Error`, `Display`, `Debug`. `::new()` and `::caused_by()` are
auto-generated. `#[from(...)]` generates `From<T>` for `?`. `#[ohno::enrich_err]`
adds file/line context.

### Spec

```yaml
types:
  Shape:
    oneof:
      Circle: { radius: float }
      Rectangle: { width: float, height: float }

  GeometryError:
    causes:
      NegativeDimension: { field: string }
      TooManyVertices: { count: int }

outcome:
  oneof: [Ok, Error, Unrecoverable]
outputs:
  when Ok:
    area: float
  when Error:
    error: GeometryError
  when Unrecoverable:
    message: string
```

### Rust mapping

```rust
// oneof → enum
pub enum Shape {
    Circle { radius: f64 },
    Rectangle { width: f64, height: f64 },
}

// causes → one ohno struct per cause, composed with #[from]
#[ohno::error]
#[display("negative dimension: {field}")]
pub struct NegativeDimension { pub field: String }

#[ohno::error]
#[display("too many vertices: {count}")]
pub struct TooManyVertices { pub count: i64 }

// #[from] also works for 3rd party errors — wraps them in the chain
#[ohno::error]
#[display("geometry error")]
#[from(NegativeDimension, TooManyVertices, std::io::Error)]
pub struct GeometryError;

// caused_by() wraps any error manually
let err = NegativeDimension::caused_by("radius", some_3rd_party_error);

// outcome oneof → Result (Unrecoverable → panic)
#[ohno::enrich_err("computing area")]
pub fn compute_area(shape: &Shape) -> Result<f64, GeometryError> { ... }
```

### Error vs Unrecoverable

| Spec outcome | Rust pattern | Test assertion |
|---|---|---|
| `when Ok` | `Result::Ok(T)` | `.unwrap()` |
| `when Error` | `Result::Err(E)` | `find_source::<T>()` on the error chain |
| `when Unrecoverable` | `panic!()` | `#[should_panic(expected = "...")]` |

Error = caller can handle it. Unrecoverable = continuing would make things worse.

```rust
#[test]
fn circle_area() {
    let area = compute_area(&Shape::Circle { radius: 5.0 }).unwrap();
    assert!((area - 78.54).abs() < 0.01);
}

#[test]
fn negative_radius() {
    use ohno::ErrorExt;
    let err = compute_area(&Shape::Circle { radius: -1.0 }).unwrap_err();
    assert!(err.find_source::<NegativeDimension>().is_some());
}

#[test]
#[should_panic(expected = "shape must not be null")]
fn null_shape_aborts() {
    compute_area_unchecked(std::ptr::null());
}
```
