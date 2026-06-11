---
name: implement-spec
description: >
  Implements a SpecGate spec file — generates source code with proper annotations, 
  a binding file, and test infrastructure. Use when asked to "implement this spec", 
  "generate code from spec", "implement <component>", or when given a .spec.yaml file 
  to implement.
---

# SpecGate Spec Implementation Skill

You are implementing a SpecGate component from its spec file. Your job is to
produce working source code with correct annotations, a binding file, and any
test fixtures needed.

## Workflow

1. **Read the spec file** the user provides or references
2. **Read `docs/knowledge/index.md`** to see what knowledge topics are available
3. **Read only the knowledge files relevant to this spec** — don't load everything
4. **Plan your implementation** based on the spec's kinds, types, and cases
5. **Generate:**
   - Source code with `spec_operation`, `spec_setup`, `spec_mock`, etc. annotations
   - A binding file (`bindings/<lang>.yaml`) if one doesn't exist
   - Test fixtures if the spec has cases requiring setup functions
6. **Validate** your annotations match the spec — every operation name in the spec
   must have a corresponding `spec_operation` in code, every test case must be
   expressible with the declared inputs

## What the spec tells you

- `name` — the component name (use for module/crate naming)
- `target` — how to build/run (you'll create this in the binding file)
- `inputs` — what the entry point takes
- `types` — type definitions (oneof = enum/union, fields = struct)
- `outcome` — what the operation returns (variants or single type)
- `outputs` — what's observable per outcome
- `cases` — concrete test cases (input → expected outcome + outputs)

## Rules

- **Every `spec_operation` needs a `kind`** — infer from the spec's structure:
  - Has `states` in the spec → StateMachine
  - Has `checkpoints` → Sequence
  - Has error variant outcomes → likely ErrorMap
  - Simple input→output → Stateless
  - No runtime behavior → Structural

- **Every `spec_setup` needs a `name`** — use the setup's purpose as the name
  (e.g., "default", "minimal", "full", "empty")

- **Every `spec_mock` needs a `name`** — use the dependency's logical name
  (e.g., "database", "http_client", "cache")

- **Setup functions must not take `self`** — they are free functions

- **Types are suggestions, not prescriptions** — the spec says what fields must be
  available, not how to structure internals. Use idiomatic language constructs.

## Knowledge base

Before implementing, read `docs/knowledge/index.md` to see available topics.
Then read only what you need:

- Always read: `spec-format.md` (understand the spec you're implementing)
- Read if placing annotations: `annotations.md`
- Read if kind is not Stateless: `kinds.md`
- Read if entry point is a method: `construction.md`
- Read if creating binding: `bindings.md`
- Read if you need validation rules: `validation.md`

## Language conventions

### Rust
- Annotations are proc macros: `#[spec_operation(...)]`
- Symbol paths use `module_path!()`: `my_crate::module::Type::method`
- Mock expansion uses `#[cfg(test)]` conditional
- Visibility: `pub(crate)` or `#[cfg(test)]` for test access

### C#
- Annotations are attributes: `[SpecOperation(...)]`
- Use `InternalsVisibleTo` for test access
- Attribute library targets `netstandard2.0` and `net9.0`

## Checklist before finishing

- [ ] Every operation in the spec has a `spec_operation` in code
- [ ] Every required role is present (State for StateMachine, Checkpoint for Sequence)
- [ ] Setup functions exist for method entry points
- [ ] Mock annotations exist for external dependencies
- [ ] Binding file exists with correct build command and output path
- [ ] Test cases from the spec are expressible with the generated code
- [ ] No `spec_setup` takes `self`
- [ ] All `spec_setup` and `spec_mock` have `name` parameters
