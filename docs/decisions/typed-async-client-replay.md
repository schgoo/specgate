# Typed generic operations and setup-backed async replay

> **Status:** accepted
> **Owner:** @schgoo (user-ratified)
> **Date:** 2026-10-21

## Context

An annotated generic client method cannot publish an open type parameter as a
concrete CTSC type. Rust replay also rejects asynchronous operations, receivers,
setup producers, and structured results.

## Decision

- Generic operation annotations explicitly declare concrete specializations.
  Each specialization has its own semantic operation name and concrete registry
  signature. Discovery must not infer supported types from whichever tests ran.
- Replay resolves annotated Rust setup producers by semantic component,
  operation, filled input, and type mappings, using the same resolution as
  discovery. File layout, class names, and test framework conventions are not
  selectors.
- Recorded setup construction inputs are passed to the selected producers.
  Their results fill operation parameters or construct the receiver. Necessary
  environment construction, including deterministic HTTP responses, belongs
  in those existing annotated producers, not in HTTP interception.
- Replay executes recorded operations with recorded inputs. Rerunning unrelated
  candidate tests is not a substitute. Results are produced by candidate code,
  never copied from the reference recording.
- Asynchronous invocation and collections of typed results retain the existing
  concrete CTSC type and comparison rules. The CLI syntax and normative document
  formats do not change.

## Consequences

### Actual Cargo test environments

The owner additionally approved discovering and replaying inside actual Cargo
test targets. Private unit-test and integration-test producers remain in their
original lexical scopes. Generated, type-erased constructor and invocation
bridges expose only the selected annotated calls to a reserved libtest entry
point. Receiver producers delegate to operation-local associated bridges, keeping
receiver downcasts and method specialization arguments in the operation's scope.
Free-function generic arguments also stay in the operation's scope, preserving authored public re-exports
rather than treating compiler `type_name` output as accessible source paths.
Cargo metadata and compiler artifacts identify targets and executables,
including explicit library crate types, custom library names, and arbitrary
integration-test source paths. Test-local operations retain their compiler-reported
crate root and target even without setup or generic arguments.
No conventional `tests/` layout or test body is used as a selector.

Discovery builds the test targets, enumerates only the generated metadata
hooks, and merges test-only declarations into the normal semantic registry.
Replay compiles the selected target and runs only its generated replay hook.
The crate-root `spec_component!` declaration emits the hidden harness macro,
which generates a libtest entry only when `SPECGATE_TEST_ENTRY` is present.
Each Cargo target needing a generated hook must declare its own component:
a library declaration does not cover separate binary or integration-test crates.
This includes operation-only targets whose annotations explicitly name their
components. Operations and setups do not emit tests beside annotated items or
inside an `impl`.
Generated test builds diagnose a missing declaration at annotated operations
and setups, including explicitly qualified declarations; ordinary builds remain
unchanged. A default component used only to install the hook is not registered
unless operations or types actually belong to it.

This owner-approved replacement, ratified on 2026-10-23 in
[#104](https://github.com/schgoo/specgate/issues/104), supersedes compiler-wrapper
crate-root injection. Builds use the original sources directly, including
read-only roots: no source mirror, sibling copy, source write, symlink,
remapping response file, helper compiler, or dependency-only compiler pass is
needed. `file!()`, inner attributes, relative modules, and includes retain
ordinary Cargo semantics. Configured compiler wrappers remain under Cargo's
normal handling, without SpecGate overriding or disabling them.

Environment dependencies emitted for `SPECGATE_TEST_ENTRY` and
`SPECGATE_TEST_DISPATCH`, together with included generated files and
module-dispatch environment dependencies, invalidate stale expansion.
Generated test builds disable incremental compilation without overriding
candidate compiler flags. Operations and setups track the entry and dispatch
variables inside their named metadata constants, which is valid inside an
`impl` and does not emit a test there.

Generated test builds use a stable, package-scoped child of Cargo's resolved
build directory, preventing generated builds from overwriting ordinary cached
artifacts. This includes explicit native library crate types, which can produce
unhashed filenames. Environment dependencies invalidate expansion but do not
change Cargo artifact filenames, so non-native targets need the same isolation.
The first generated build can recompile dependencies once; later builds reuse
them within that namespace.

External discovery and replay runners also use isolated build namespaces,
keyed by their candidate and generated Cargo files. Ordinary capture builds
can select different dependency features or versions from those runners;
sharing unhashed native libraries across those graphs can link two runtime
instances even when their runtime package identity is the same. Runner
namespaces preserve dependency-cache reuse without consuming those artifacts.

The owner-approved concurrency policy rejects overlapping SpecGate commands
against the same canonical package sources immediately, rather than waiting
or creating per-invocation build namespaces. `discover`, `capture`, and `replay`
acquire an operating-system file lease before discovery and hold it through
enumeration, execution, and publication. Reusable replay candidates retain the
lease until their last shared handle is dropped. The lock file lives outside
the sources, is never unlinked, and does not remain locked after the process
exits. Different packages remain independent. This is command-level ownership,
not a change to libtest threading or native operation concurrency; ordinary
`cargo test` retains parallel test execution.

Each scenario owns a separate scope so its receiver and setup
guards are dropped before the next scenario starts.

Structured outcomes retain the exact reference registry bytes for linked
validation before decoding and before publishing candidate output. Reachable
named declarations, including those reached only through observed nested calls,
must match; known tuple-versus-record or ownership
differences require explicitly authored canonical adapters, not implicit
structural mappings. Direct replay across those different declarations remains
unsupported. Validation
failure never replaces an existing output.

Missing or ambiguous producers and undeclared generic specializations must fail
explicitly. An annotation does not make arbitrary unannotated test environment
reproducible. Setup code must construct everything the replayed operation needs.

Rust specializations must preserve the authored return signature's syntactic
completion shape. Substitutions that change unit, optional-empty, or
success/error channels are rejected before instrumentation; for example,
`Option<T>` with `T = ()`, or `T` with `T = Result<i32, String>`. Such operations
must declare a concrete return signature. This check does not resolve aliases
or associated-type projections.

## Validation

Focused discovery, capture, planner, and generated-runner tests must exercise
semantic input renaming, receiver and parameter setup resolution, async
completion, and concrete generic typed results. Captures and replay outputs must
pass the native CTSC validators.

## Candidate-owned execution (2026-10-22)

The owner approved keeping standard engine selection in the binding and custom
runtime configuration in ordinary candidate code, with Arty as a first-class
integration alongside Smol and Tokio. The recording crate `specgate-runtime`
must not depend unconditionally on any async engine.

The previously unused `rust.runtime` field now selects replay execution.
Its default is `auto`, selecting exactly one supported direct candidate
dependency, including renamed and development dependencies. Ambiguity is an
error, not permission to substitute a different runtime. Generated external
manifests preserve the selected dependency's exact resolved version,
enabled features, and supported path/registry source, without implicitly enabling
default features; generated test entries use the candidate's
dependency directly.

Generated invocation dispatch is selected by compiler-reported module identity
before type checking. Producer-local projections retain owned setup values when
borrowing unsized parameters such as `String` into `&str`; operation-local
bridges continue to resolve operation types and generic specializations.
Replay rejects consuming receivers during planning because receiver reuse cannot
transfer ownership safely. Borrowed `&self` and `&mut self` methods remain
supported. Library, binary-unit, and integration-test targets retain their Cargo
target kinds. Operation-only targets receive the same crate-root discovery
hook through their local `spec_component!` declaration; no setup is required.

An optional `rust.executor` names a crate-rooted candidate wrapper. It replaces
built-in execution and owns the complete task factory, including construction
and teardown of setup-owned environments. A factory, rather than an already
constructed future, lets Arty create non-`Send` futures and receivers on the
worker that will poll them. Custom wrappers can reuse native-test configuration
helpers and provide application-owned Arty task capabilities without SpecGate
inventing a global runtime context. Capture continues to run the original tests
with their original wrappers. Synchronous replay requires no async engine.

## Canonical adapters and structured inputs

**Accepted direction; general structured-input reconstruction is not implemented.**
The owner selected design-first work for structured inputs and cross-language
shape differences, with candidate-code adapters exposing one shared CTSC
contract. The later [constructor and property setup decision](constructor-and-property-setups.md)
implements a narrower opt-in and recursive collection inputs.

Adapters are ordinary annotated operation boundaries. They expose canonical
contract types, convert canonical arguments to application types, invoke the
application, and convert its results and declared errors back to the canonical
types. For example, a Rust application returning `(id, label)` and a C#
application returning a record can each expose a canonical record with named
`id` and `label` fields. Both annotated boundaries must publish the same CTSC
declarations and record those canonical values; replay does not equate an
already-recorded tuple with a record.

Naming-only differences should first use existing semantic type and field names.
Other conversions belong in explicit code, where ownership, validation, and
failure behavior can be reviewed and tested. Adapters must preserve all data
covered by the shared contract. There is no binding mapping language, hidden
coercion, rewriting of reference evidence, or change to `ctsc.strict/0.1.0`.
Native application declarations do not need to change.

Structured input support requires a separate, opt-in, fallible reconstruction
contract. Capture projection alone is insufficient: `SpecEvent` can select only
some fields, and those fields may not reconstruct a valid application value.
Projection alone must not fill omitted fields with `Default`. The separately
ratified [constructor and property setup decision](constructor-and-property-setups.md)
allows defaulting only for explicitly opted-in property setups: that opt-in
promises omitted fields may safely be defaulted. Types with construction
invariants must use explicit constructors or adapters.

The planned reconstruction path must:

- Validate reference values and matching candidate declarations before running
  setup producers or operations, reporting a precise input/field path on failure.
- Support complete, recursively owned records, tagged unions, lists, tuples,
  optionals, sets, and string-keyed maps while preserving primitive ranges,
  signedness, floating-point behavior, and collection semantics.
- Keep decoding and invocation inside lexical bridges so private and test-only
  types remain accessible. Borrowed arguments require owned backing values
  whose lifetimes cover the invocation, including asynchronous completion.
- Carry decoded values through the OTLP decoder, replay plan, generated runner,
  and runtime bridge rather than merely removing the existing input rejection.

The exact public reconstruction API is not ratified here. Until that vertical
slice is implemented, general record and tagged-union operation inputs remain unsupported and
canonical structured input adapters are a design, not an advertised workaround.

## C# specialization validation and caching

Discovery validates each declared concrete specialization with the CLR's
`MakeGenericMethod` and reports rejected constraints with component, operation,
method, and type arguments. This is runtime constraint validation, not a claim
to reproduce every C# compiler-only rule: nullable annotations, for example,
do not survive as CLR constraints.

Capture caches successful specialization selection by concrete method handle,
component, and declared operation identity, independently of semantic value
projection. Disabled capture still performs no specialization reflection.
An undeclared specialization continues to run normally outside capture and
fails explicitly when captured, before unsupported value projection.
