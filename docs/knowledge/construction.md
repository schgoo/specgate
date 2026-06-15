# Construction resolution and setup

When an operation's entry point is a method (takes `self`/`this`), the harness
needs to construct the owning type. This is where `spec_setup` comes in.

## Resolution algorithm

1. **Entry point** — what types does it need? (self type, parameter types)
2. **For each type, try in order:**
   - Find a `spec_setup` that returns this type → use it
   - Auto-discover a single public constructor → use it, recurse on its params
   - Primitive/leaf type → provided by test case inputs
3. **If unresolvable** — validation error

All resolved constructor/setup parameters bubble up as flat test case inputs.

## spec_setup

Setup functions are test fixtures. They live in test code, not production code.

```rust
// Rust
#[spec_setup("canvas", name = "default")]
fn make_canvas(width: f64, height: f64) -> Canvas {
    Canvas::new(width, height)
}
```

```csharp
// C#
[SpecSetup("canvas", Name = "default")]
public static Canvas MakeCanvas(double width, double height)
    => new Canvas(width, height);
```

### Rules

- **No `self`/`this`** — setups are free functions, never methods
- **`name` is required** — the language-agnostic join key referenced from spec cases
- **Names must be unique** per operation
- **Multiple setups allowed** — different setups can return different types for the same operation

### Environment setup

A setup that returns `()` / `void` configures environment rather than constructing a type:

```rust
#[spec_setup("canvas", name = "runtime")]
fn init_renderer(backend: String) {
    std::env::set_var("RENDER_BACKEND", backend);
}
```

### Ambiguous construction warning

If the entry point is a method but no `spec_setup` exists, `core.validate`
emits an `AmbiguousConstruction` warning.

## Full example

```rust
struct Canvas {
    #[spec_capture("canvas")]
    shapes: Vec<Shape>,
    #[spec_capture("canvas")]
    total_area: f64,
}

impl Canvas {
    fn new(width: f64, height: f64) -> Self { ... }

    #[spec_operation("canvas", kind = StateMachine)]
    fn add_shape(&mut self, shape: Shape) { ... }

    #[spec_mock("canvas", name = "renderer")]
    fn render(&self) -> Vec<u8> { ... }
}

#[spec_setup("canvas", name = "default")]
fn make_canvas(width: f64, height: f64) -> Canvas {
    Canvas::new(width, height)
}
```

The harness resolves: `add_shape` needs `Canvas` → found `make_canvas`
→ needs `width: f64, height: f64` (primitives) → they become test case inputs.
