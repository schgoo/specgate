# CTSC Comparison 0.1

**Status:** Draft

## 1. Scope

CTSC defines trace and registry interchange. It does not require every
comparator to use one equality algorithm.

This document defines:

- the semantic information available to comparison policies;
- the policy decisions a comparator must declare;
- requirements for deterministic and explainable results;
- an optional CTSC Strict reference policy.

A producer may emit conforming CTSC artifacts without implementing comparison.
A comparator may implement one or more policies.

## 2. Input handling

A comparator MUST accept CTSC Trace Core artifacts and MAY support CTSC Linked
comparison with registry documents.

Before comparison, it MUST either:

- validate each input according to its CTSC conformance level; or
- require independently validated inputs and report that assumption.

Invalid artifacts are validation failures, not behavioral differences.

A comparator MUST preserve the source artifacts. It MAY derive internal indexes,
trees, typed values, or normalized views.

## 3. Semantic input model

Comparison policies operate on the following CTSC semantics:

- runs containing scenarios;
- scenario, operation, and parallel span hierarchy;
- operation component IDs, names, and inputs;
- event-array order;
- observations and their values;
- unit, result, empty, and declared-error completion;
- target and supervisor faults;
- concrete OTLP `AnyValue` structures;
- registry record, tagged-union, tuple, set, map, and numeric types;
- timestamps, durations, resources, scopes, attributes, and extensions.

Trace and span IDs identify and connect spans within one execution. Independent
executions generate different IDs, so comparators use them to reconstruct each
trace hierarchy but do not compare the ID values across executions.

## 4. Policy declaration

Every comparison result MUST identify the comparison policy and its version.
When policy options are configurable, the result MUST include or reference the
effective configuration.

A policy MUST define:

| Dimension | Questions the policy answers |
|---|---|
| Run selection | Which run from each artifact is compared? |
| Scenario matching | Are scenarios paired by name, position, selection, or another key? |
| Operation matching | Are repeated invocations paired by order, inputs, identity, or another rule? |
| Event ordering | Must event-array order match? |
| Presence | Are additional/missing scenarios, operations, observations, or results allowed? |
| Values | Which values require exact equality, projection, coercion, or custom matching? |
| Collections | Does the policy preserve or explicitly relax declared list, tuple, set, and map semantics? |
| Floating point | Exact, absolute tolerance, relative tolerance, or another policy? |
| Faults | Which fault fields participate in equality? |
| Parallel regions | How are unordered branches paired? |
| Timing | Are timestamps or durations compared? |
| Resources and scopes | Which resource/scope attributes participate? |
| Extensions | Which extension namespaces participate? |

Policies MAY expose additional dimensions.

## 5. Determinism

Given the same artifacts and effective policy configuration, a comparator MUST
produce the same verdict and semantic mismatch locations.

A policy relying on custom callbacks, external state, or nondeterministic
matching MUST define how determinism is maintained or report that the result is
non-portable.

## 6. Result requirements

A comparison result MUST identify:

- the selected input artifacts and runs;
- the comparison policy and version;
- whether the artifacts are equivalent under that policy;
- validation failures, if any;
- semantic mismatch locations.

A semantic mismatch location SHOULD include:

- scenario;
- operation path;
- event or child-span position;
- expected and actual semantic values.

Diagnostic wording and serialization format are not defined by CTSC 0.1.

## 7. CTSC Strict reference policy

`ctsc.strict/0.1.0` is an optional reference policy. Implementing CTSC does not
require implementing this policy.

### 7.1 Run selection

The caller selects one run from each artifact. Selection is implicit only when
each artifact contains exactly one run.

`conformance.run.id` is provenance and does not participate in equality.

### 7.2 Scenario matching

Scenarios pair by `conformance.scenario.name`. Names must be unique within a
run.

Missing or additional scenarios are mismatches.
`conformance.scenario.index` does not participate in equality.

### 7.3 Sequential operation matching

Children of ordinary run, scenario, and operation spans are ordered by their
non-overlapping time intervals and pair by position.

Paired operation spans must have equal:

- `conformance.component.id`;
- `conformance.operation.name`;
- operation inputs.

Repeated invocations of the same operation pair by position.

Overlapping child spans outside `conformance.parallel` are unsupported by this
policy.

### 7.4 Events

Events pair by position in the OTLP event array. Event names and required CTSC
attributes must match.

Missing, additional, or reordered events are mismatches.

### 7.5 Completion

Unit, result, empty, declared error, and fault are distinct termination states.

Results and declared errors compare their values.

### 7.6 Values

Trace Core values compare by exact structural representation:

- scalar `AnyValue` variants must match;
- arrays compare in order;
- key/value lists compare by unique key, independent of entry order.

Linked values additionally use registry semantics:

- records require the same declared fields;
- tagged unions require the same variant and equivalent payload;
- lists compare by position;
- tuples compare by position;
- sets compare as unordered duplicate-free collections;
- maps compare as unordered key/value collections;
- integer widths and ranges must be valid.

Custom policies MAY relax collection equality, such as treating a list as
unordered, but MUST declare that deviation from the registry type semantics.

The Strict policy compares valid floating-point values exactly. Other policies
may define tolerance.

### 7.7 Faults

Faults pair only with faults. The Strict policy compares:

- `conformance.fault.type`;
- `conformance.fault.message`, including presence or absence.

It ignores:

- `conformance.fault.native_type`;
- `conformance.fault.observer`;
- process exit code, signal, timeout, phase, and operation-attribution
  diagnostics.

### 7.8 Parallel regions

Direct children of `conformance.parallel` are unordered.

Branches with unique semantic identities pair by fixed span name and, for
operations:

```text
(conformance.component.id, conformance.operation.name)
```

The Strict policy reports duplicate branches with the same semantic identity as
ambiguous rather than selecting a pairing.

Each paired branch is then compared recursively using the Strict policy.

### 7.9 Timing and metadata

The Strict policy uses timestamps only to establish sequential child-span order.
It does not compare:

- absolute timestamps;
- durations;
- trace IDs or span IDs;
- OTLP batch or file position;
- resource attributes other than required CTSC registry linkage;
- instrumentation scope details;
- producer-specific extensions.

## 8. Custom policies

A custom policy may relax, strengthen, or replace Strict behavior. Examples
include:

- selecting a scenario subset;
- allowing additional observations;
- matching repeated operations by inputs;
- applying floating-point tolerances;
- comparing only fault occurrence;
- pairing duplicate parallel branches by application-specific keys;
- comparing performance durations;
- including selected extension namespaces.

Custom policy behavior is interoperable only when its identity, version, and
effective configuration are shared with every participant interpreting the
result.
