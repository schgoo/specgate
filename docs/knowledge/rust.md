# Rust implementation conventions

## Project structure

```
crates/<component-name>/
  Cargo.toml
  src/lib.rs
  tests/spec_tests.rs   # optional — for hand-written companion tests
```

Add the crate to the workspace `Cargo.toml` if one exists. The
fixture crate (`test/rust/crates/specgate-fixtures/`) is the running
example.

## Common dependencies

- `specgate-annotations` — provides the five `#[spec_*]` proc macros and
  the `spec_event!()` macro.
- `serde` + `serde_yaml` — when the implementation parses spec/binding files.
- `ohno` — error types (project convention — never use `thiserror` /
  `anyhow`).
- `clap` — only if building a CLI.

## Annotations

```rust
use specgate_annotations::*;

// Pure function operation
#[spec_operation("add")]
fn add(a: i32, b: i32) -> i32 { a + b }

// Setup constructs the system-under-test
#[spec_setup("make_counter")]
fn make_counter() -> Counter { Counter { count: 0 } }

struct Counter {
    #[spec_event]
    count: i32,
}

impl Counter {
    // Method operation
    #[spec_operation("increment")]
    fn increment(&mut self) { self.count += 1; }
}

// Inline checkpoint
#[spec_operation("process")]
fn process(data: &str) -> String {
    let upper = data.to_uppercase();
    spec_event!("after_upper", &upper);
    upper.trim().to_string()
}

// Mock — replaces a call with a case-supplied response
struct UserService { db: RealDb }
impl UserService {
    #[spec_operation("get_user")]
    fn get_user(&self, id: &str) -> String {
        #[spec_mock("db")]
        let response = self.db.find(id);
        response
    }
}
```

Every fixture under `test/rust/crates/specgate-fixtures/src/` is a
copy-paste-ready example.

## Return value conventions

| Return shape | Trace events emitted | Spec asserts |
|--------------|----------------------|--------------|
| `T` (any value) | `Event { "<op>.result", value }` | `- <op>.result: "<value>"` |
| `Result<T, E>` Ok arm | `Event { "<op>.outcome", "Ok" }` + `Event { "<op>.result", value }` | `- <op>.outcome: "Ok"` + result |
| `Result<T, E>` Err arm | `Event { "<op>.outcome", "Error" }` + `Event { "<op>.error", msg }` | `- <op>.outcome: "Error"` + error |
| `panic!()` | `Event { "<op>.error", panic_msg }` | `- <op>.error: "<msg>"` |
| `()` / no return | no `.result` event | rely on `#[spec_event]` field events |

See `result_ok.rs`, `result_err.rs`, `unrecoverable.rs`, `void_operation.rs`.

## Field naming

A `#[spec_event]` field emits events under the bare field name in
single-setup cases (`count`, `balance`). In multi-setup cases the
case's alias becomes a prefix (`source.balance`, `target.balance`).

## Error handling

Use `ohno` for any error types you author. Never use `thiserror` /
`anyhow`.
