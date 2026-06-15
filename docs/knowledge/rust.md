# Rust implementation conventions

## Project structure

```
crates/<component-name>/
  Cargo.toml
  src/lib.rs
  tests/spec_tests.rs
```

Add the crate to the workspace `Cargo.toml` if one exists.

## Common dependencies

- `serde` + `serde_json` + `serde_yaml` — serialization
- `ohno` — error types (project convention — never use thiserror/anyhow)
- `clap` — CLI argument parsing (if building a CLI)

## Annotations

Rust uses proc macros from `specgate-annotations`.

```rust
// Stateless operation — pure input → output
#[spec_operation("area", kind = Stateless)]
fn compute_area(shape: &Shape) -> Result<f64, GeometryError> { ... }

// Setup — constructs inputs for tests (no self)
#[spec_setup("area", name = "circle")]
fn make_circle(radius: f64) -> Shape {
    Shape::Circle { radius }
}

// Capture on struct — all public fields captured
#[spec_capture("area")]
struct AreaResult {
    pub area: f64,
    pub perimeter: f64,
}

// Capture on individual fields
struct Canvas {
    #[spec_capture("canvas")]
    total_area: f64,
    internal_buffer: Vec<u8>,  // not captured
}

impl Canvas {
    // StateMachine operation
    #[spec_operation("canvas", kind = StateMachine)]
    fn add_shape(&mut self, shape: Shape) { ... }

    // Mock — external dependency
    #[spec_mock("canvas", name = "renderer")]
    fn render(&self) -> Vec<u8> { ... }

    // Checkpoint — intermediate value
    #[spec_checkpoint("canvas")]
    fn current_bounds(&self) -> (f64, f64) { ... }
}

// Inline checkpoint — captures any expression
#[spec_operation("pipeline", kind = Sequence)]
fn process(input: &str) -> Output {
    let validated = spec_checkpoint!("pipeline", validator.check(input));
    Output { validated }
}
```

Symbol paths are fully qualified: `my_crate::module::Type::method`.

## Mapping spec types to Rust

| Spec type | Rust type |
|-----------|-----------|
| `string` | `String` |
| `int` | `i64` |
| `float` | `f64` |
| `bool` | `bool` |
| `List<T>` | `Vec<T>` |
| `Option<T>` | `Option<T>` |
| `Map<K, V>` | `BTreeMap<K, V>` |
| `oneof` | `enum` |
| `causes` | ohno structs (see below) |
| record | `struct` |

## Spec to Rust mapping example

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
      UnsupportedShape: { name: string }

outcome:
  oneof: [Ok, Error]
outputs:
  when Ok:
    area: float
  when Error:
    error: GeometryError
```

### Rust

```rust
// oneof → enum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Shape {
    Circle { radius: f64 },
    Rectangle { width: f64, height: f64 },
}

// causes → ohno structs
#[ohno::error]
#[display("negative dimension: {field}")]
pub struct NegativeDimension { pub field: String }

#[ohno::error]
#[display("unsupported shape: {name}")]
pub struct UnsupportedShape { pub name: String }

#[ohno::error]
#[display("geometry error")]
#[from(NegativeDimension, UnsupportedShape)]
pub struct GeometryError;

// outcome → Result
pub fn compute_area(shape: &Shape) -> Result<f64, GeometryError> { ... }
```

## Generated tests

Each spec case maps to one `#[test]`:

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
```

## Error handling

Use `ohno` for all error types. Never use thiserror/anyhow.

| Spec outcome | Rust pattern | Test assertion |
|---|---|---|
| `when Ok` | `Result::Ok(T)` | `.unwrap()` |
| `when Error` | `Result::Err(E)` | `find_source::<T>()` |
| `when Unrecoverable` | `panic!()` | `#[should_panic(expected = "...")]` |
