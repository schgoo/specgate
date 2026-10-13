using System;
using System.Collections.Generic;

namespace SpecGate.Runtime;

/// <summary>
/// The live state of one operation boundary between entry and termination.
/// </summary>
/// <remarks>
/// Woven code treats this as an opaque token. The type is a reference type on
/// purpose: <see cref="SpecGateRuntime"/> publishes it through an
/// <c>AsyncLocal</c>, so a continuation resuming on another thread observes the
/// same instance rather than a thread-affine copy.
/// </remarks>
public sealed class OperationScope
{
    private readonly List<RecordedValue> _inputs = [];

    internal OperationScope(
        CaptureSession? session,
        OperationScope? parent,
        long spanId,
        string component,
        string operation,
        RuntimeMethodHandle method)
    {
        Session = session;
        Parent = parent;
        SpanId = spanId;
        Component = component;
        Operation = operation;
        Method = method;
    }

    internal CaptureSession? Session { get; }

    internal OperationScope? Parent { get; }

    /// <summary>The woven method, used to read what its values mean.</summary>
    internal RuntimeMethodHandle Method { get; }

    internal bool Completed { get; private set; }

    /// <summary>Gets the session-unique span identifier, or zero when not recording.</summary>
    public long SpanId { get; }

    /// <summary>Gets the owning component identifier.</summary>
    public string Component { get; }

    /// <summary>Gets the semantic operation name.</summary>
    public string Operation { get; }

    /// <summary>Gets whether this scope is attached to an active capture session.</summary>
    public bool IsRecording => Session is not null;

    internal static OperationScope Disabled { get; } =
        new OperationScope(null, null, 0, string.Empty, string.Empty, default);

    /// <summary>
    /// Projects and records the operation's folded inputs: the construction
    /// inputs of the setups it requires, plus every declared parameter no setup
    /// contributes in its place.
    /// </summary>
    /// <remarks>
    /// Inputs are recorded sorted by name, not in parameter order, because Rust
    /// capture stores them in a <c>BTreeMap&lt;String, Value&gt;</c>
    /// (<c>rust/crates/specgate-runtime/src/capture.rs:67-85</c>) and the two
    /// producers have to agree on the CTSC-visible order.
    /// </remarks>
    internal void RecordInputs(object?[] arguments)
    {
        if (Session is null)
        {
            return;
        }

        SemanticInput[] declared = OperationMetadata.For(Method).Inputs;
        if (declared.Length != arguments.Length)
        {
            throw new SpecGateCaptureException(
                CaptureRejection.TypeMismatch,
                $"specgate: operation '{Operation}' received {arguments.Length} argument(s) but "
                + $"declares {declared.Length}.");
        }

        SetupFolding folding = SetupFolding.For(Method, Component, Operation, declared);
        SortedDictionary<string, Value> folded = new(Utf8KeyComparer.Instance);
        Dictionary<string, string> producers = new(StringComparer.Ordinal);

        foreach (FoldedSetup setup in folding.Required)
        {
            SortedDictionary<string, Value> inputs = Session.SetupInputs(setup.Key)
                ?? throw new SpecGateCaptureException(
                    CaptureRejection.SetupMissing,
                    $"specgate: capture for '{Component}::{Operation}' requires successful provenance "
                    + $"from {setup.Description}; invoke that [SpecSetup] during this capture and let it "
                    + $"return successfully before calling '{Operation}'.");

            foreach (KeyValuePair<string, Value> input in inputs)
            {
                Merge(folded, producers, input.Key, input.Value, setup.Description);
            }
        }

        for (int index = 0; index < declared.Length; index++)
        {
            SemanticInput input = declared[index];
            Value value = ValueProjector.Project(input.Type, arguments[index]);
            if (folding.Filled.Contains(input.Name))
            {
                // A filled argument is projected and then discarded, mirroring
                // the generated Rust code at
                // `rust/crates/specgate-runtime/src/capture.rs:179-202`, where the
                // value is projected before `input()` drops the filled name. An
                // unrepresentable value in a filled position has to fail in both
                // producers, not just one.
                continue;
            }

            Merge(folded, producers, input.Name, value, $"operation parameter '{input.Name}'");
        }

        lock (_inputs)
        {
            foreach (KeyValuePair<string, Value> input in folded)
            {
                _inputs.Add(new RecordedValue(input.Key, input.Value));
            }
        }
    }

    /// <remarks>
    /// Rust asserts here, because a folded surface with two inputs of one name is
    /// a metadata fault that discovery already rejects
    /// (<c>discovery/setup.rs:160-171</c>). Capture names both producers so the
    /// fault is actionable.
    /// </remarks>
    private void Merge(
        SortedDictionary<string, Value> folded,
        Dictionary<string, string> producers,
        string name,
        Value value,
        string producer)
    {
        if (producers.TryGetValue(name, out string? earlier))
        {
            throw new SpecGateCaptureException(
                CaptureRejection.AmbiguousDeclaration,
                $"specgate: setup folding for '{Component}::{Operation}' produces duplicate input name "
                + $"'{name}'; {earlier} collides with {producer}.");
        }

        producers.Add(name, producer);
        folded.Add(name, value);
    }

    /// <summary>Projects the operation's return value, or returns null when it is void.</summary>
    internal RecordedValue? ProjectResult(object? result)
    {
        SemanticType? declared = OperationMetadata.For(Method).Result;
        return declared is null
            ? null
            : new RecordedValue(OperationMetadata.ResultName, ValueProjector.Project(declared, result));
    }

    internal void Complete(SpanStatus status, RecordedValue? result, Exception? exception)
    {
        if (Session is null || Completed)
        {
            return;
        }

        Completed = true;

        RecordedValue[] inputs;
        lock (_inputs)
        {
            inputs = [.. _inputs];
        }

        Session.Record(new RecordedSpan(
            SpanId,
            Parent?.SpanId,
            Component,
            Operation,
            inputs,
            status,
            result,
            exception?.GetType().FullName,
            exception?.Message));
    }
}
