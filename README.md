# SpecGate

Deterministic spec verification for annotated code. Annotate your code with SpecGate attributes, run the extraction pipeline, and get a machine-readable spec (YAML) + validation diagnostics.

## Quick start

### 1. Annotate your C# code

Add a reference to `SpecGate.Annotations` and annotate your code:

```csharp
using SpecGate;

public class UserService
{
    [SpecInput("findUserByKey")]
    public UserService(IDatabase db) { ... }

    [SpecOperation("findUserByKey", SpecKind.Pure)]
    public User FindByKey(string key) { ... }

    [SpecEnvironment("findUserByKey")]
    public string TenantId { get; set; }

    [SpecDependency("findUserByKey", Dep = "database")]
    public IDatabase Database => _db;
}
```

### 2. Extract

```bash
# Step 1: C# front-end → intermediate JSON
cd csharp/SpecGate.Extractor
dotnet run -- <path-to-your.csproj> --output extraction.json

# Step 2: Rust core → spec YAML + diagnostics
cd rust
cargo run --bin specgate-extract -- --input extraction.json --output specs/
```

### 3. Review output

The pipeline produces:
- **`specs/<operation>.spec.yaml`** — machine-readable spec for each operation
- **Diagnostics on stderr** — validation errors and warnings

Example output (`specs/findUserByKey.spec.yaml`):
```yaml
name: findUserByKey
kind: Pure
inputs:
- name: db
  type: IDatabase
environments:
- name: TenantId
  type: str
dependencies:
- name: Database
  type: IDatabase
  dep: database
```

Example diagnostics:
```
error[SG005]: Type 'IDatabase' is abstract/interface — needs a [SpecGenerator]
1 error(s), 0 warning(s), 0 note(s) — 1 operation(s) found
```

## Project structure

```
specgate/
├── schema/
│   ├── spec-schema.json          # JSON Schema for .spec.yaml files
│   └── extraction-schema.json    # JSON Schema for intermediate format
├── csharp/
│   ├── SpecGate.Annotations/     # C# attribute library (8 attributes + enum)
│   ├── SpecGate.Extractor/       # Roslyn-based front-end
│   └── SpecGate.TestSubject/     # Example annotated project
├── rust/
│   ├── specgate-core/            # Shared validation + emission library
│   └── specgate-extract/         # CLI binary
├── specs/                        # Generated spec files
├── docs/
│   └── csharp-annotation-prompt.md
└── .github/skills/
    ├── specgate-annotate/        # Copilot skill for annotating code
    └── specgate-review/          # Copilot skill for reviewing annotations
```

## Architecture

```
Annotated C# code          Annotated Rust code (future)
       │                            │
       ▼                            ▼
  C# Extractor              Rust Extractor (future)
  (Roslyn)                   (syn)
       │                            │
       └──── extraction.json ───────┘
                    │
                    ▼
            specgate-core (Rust)
            - validate annotations
            - check constructability
            - map types
            - emit diagnostics
                    │
                    ▼
            .spec.yaml + diagnostics
```

Language-specific front-ends are thin — they walk the AST and emit intermediate JSON. The shared Rust core does all validation, type mapping, and spec generation.

## Diagnostic codes

| Code  | Severity | Meaning |
|-------|----------|---------|
| SG001 | Error    | Annotation references non-existent operation |
| SG002 | Error    | Annotation on invalid target for its role |
| SG003 | Warning  | Operation has no inputs |
| SG004 | Warning  | `[SpecState]` on non-StateMachine operation |
| SG005 | Error    | Type is unconstructable (no public ctor, no `[SpecGenerator]`) |
| SG006 | Error    | `[SpecGenerator]` method is not static |
| SG007 | Error    | `[SpecContext]` method is not static |
| SG008 | Error    | Duplicate operation with conflicting kinds |
| SG009 | Error    | `[SpecOperation]` missing `kind` argument |
| SG010 | Error    | Annotation missing `name` argument |
| SG011 | Error    | `[SpecCheckpoint]` on non-method symbol |
| SG012 | Note     | Type referenced but not in extraction type info |

## Attributes

| Attribute | Placed on | Purpose |
|-----------|-----------|---------|
| `[SpecOperation("name", Kind)]` | Method | Entry point for a spec operation |
| `[SpecInput("name")]` | Constructor, property, method | Input to the operation |
| `[SpecCheckpoint("name")]` | Method | Observable intermediate value |
| `[SpecState("name")]` | Field, property | State snapshot (StateMachine only) |
| `[SpecEnvironment("name")]` | Property, method | Ambient state reader |
| `[SpecDependency("name", Dep="x")]` | Property, method | External dependency |
| `[SpecGenerator("TypeName")]` | Static method | Factory for unconstructable types |
| `[SpecContext("name")]` | Static method | Wires spec primitives to ambient state |

## Building

```bash
# Rust
cd rust && cargo build

# C#
cd csharp/SpecGate.Annotations && dotnet build
cd csharp/SpecGate.Extractor && dotnet build

# Tests
cd rust && cargo test
```
