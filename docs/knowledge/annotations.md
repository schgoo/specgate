# Annotation syntax

Annotations link source code symbols to spec operation names. Each annotation
contributes a piece to an operation — the harness collects all annotations
sharing the same operation name.

## Rust (proc macros)

| Macro | Placed on | Purpose |
|-------|-----------|---------|
| `#[spec_operation("name", kind = K)]` | Entry point method | Marks the function target. Kind is required. |
| `#[spec_setup("op", name = "n")]` | Free function or constructor | Constructs objects or configures environment. No `self`. Name is required. |
| `#[spec_checkpoint("name")]` | Internal method | Observable intermediate state (Sequence kind only) |
| `#[spec_state("name")]` | Struct field | State to snapshot (StateMachine kind only) |
| `#[spec_mock("op", name = "n")]` | Method calling external service | Makes function mockable via `cfg(test)` conditional |

### Rules

- `spec_operation` requires both the operation name and `kind`
- `spec_setup` and `spec_mock` require both the operation name and `name`
- `spec_setup` must NOT take `self` — it's a free function or associated function
- `spec_state` must be placed on a struct field, not a function
- `spec_checkpoint` must be placed on a method, not a field
- Only one `spec_operation` per operation name per crate (duplicates are extracted but fail validation)
- `spec_setup` names must be unique per operation within a crate

### Symbol resolution

Proc macros use `module_path!()` to produce fully qualified symbols:
```
concat!(module_path!(), "::", "method_name")
→ "my_crate::handlers::Request::new"
```

Nested modules produce deeper paths:
```
mod inner { mod deep { fn handler() {} } }
→ "my_crate::inner::deep::handler"
```

### async and generic functions

Both work normally with all annotations. The macro extracts the same metadata
regardless of whether the function is async or generic.

### spec_mock expansion

The macro wraps the function body to check for a registered mock in test mode:

```rust
fn get_user(&self, id: &str) -> User {
    #[cfg(test)]
    if let Some(mock) = SPECGATE_MOCKS.get("user_service") {
        return mock.call(id);
    }
    // original implementation
}
```

## C# (attributes)

| Attribute | Placed on | Purpose |
|-----------|-----------|---------|
| `[SpecOperation("name", Kind = K)]` | Entry point method | Marks the function target |
| `[SpecSetup("op", Name = "n")]` | Static method or constructor | Constructs objects or configures environment |
| `[SpecCheckpoint("name")]` | Internal method | Observable intermediate state |
| `[SpecState("name")]` | Property or field | State to snapshot |
| `[SpecMock("op", Name = "n")]` | Method calling external service | Makes function mockable |

C# uses Roslyn source generators (compile-time, deterministic) for instrumentation.
The attribute library targets both `netstandard2.0` and `net9.0`.
