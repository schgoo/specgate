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
    internal OperationScope(
        CaptureSession? session,
        OperationScope? parent,
        PendingOperation? pending,
        string component,
        string operation,
        RuntimeMethodHandle method)
    {
        Session = session;
        Parent = parent;
        Pending = pending;
        Component = component;
        Operation = operation;
        Method = method;
    }

    internal CaptureSession? Session { get; }

    internal OperationScope? Parent { get; }

    /// <summary>The span being recorded, or <see langword="null"/> when not recording.</summary>
    internal PendingOperation? Pending { get; }

    /// <summary>The woven method, used to read what its values mean.</summary>
    internal RuntimeMethodHandle Method { get; }

    internal bool Completed { get; private set; }

    /// <summary>Gets the span identifier, or <see langword="null"/> when not recording.</summary>
    public SpanId? SpanId => Pending?.SpanId;

    /// <summary>Gets the owning component identifier.</summary>
    public string Component { get; }

    /// <summary>Gets the semantic operation name.</summary>
    public string Operation { get; }

    /// <summary>Gets whether this scope is attached to an active capture session.</summary>
    public bool IsRecording => Session is not null && Pending is not null;

    internal static OperationScope Disabled { get; } =
        new OperationScope(null, null, null, string.Empty, string.Empty, default);

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
        if (Session is null || Pending is null)
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

        Pending.RecordInputs(folded);
        Session.Persist();
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

    /// <summary>
    /// Projects the operation's return value onto its terminal channel, or
    /// returns null when it is void.
    /// </summary>
    internal ReturnOutcome? ProjectResult(object? result)
    {
        SemanticType? declared = OperationMetadata.For(Method).Result;
        return declared is null ? null : ValueProjector.ProjectReturn(declared, result);
    }

    /// <summary>
    /// Completes the span on the channel the return value selected, or on no
    /// channel at all when the operation returns nothing.
    /// </summary>
    /// <remarks>
    /// A void operation reports no completion event, matching Rust's
    /// <c>NativeTerminal::Unit</c>: the span is still <c>Ok</c>, but there is no
    /// terminal value to record. An absent optional is the explicit
    /// <c>empty</c> channel, and a failed result is the <c>error</c> channel.
    /// </remarks>
    internal void CompleteValue(ReturnOutcome? outcome)
    {
        if (!TryBeginCompletion())
        {
            return;
        }

        Func<ulong, long, Completion>? completion = outcome switch
        {
            null => null,
            { Channel: ReturnChannel.Empty } => (order, time) => new EmptyCompletion(order, time),
            { Channel: ReturnChannel.Error } returned => (order, time) =>
                new ErrorCompletion(order, time, OperationMetadata.ErrorName, returned.Value),
            { } returned => (order, time) => new ResultCompletion(order, time, returned.Value!),
        };
        Session!.Complete(Pending!, completion);
    }

    /// <summary>
    /// Completes the span for an operation that terminated by throwing.
    /// </summary>
    /// <remarks>
    /// An exception the operation declares through <c>[SpecException]</c> is its
    /// semantic error outcome, so it is recorded on the <c>error</c> channel with
    /// the message as its value, exactly as Rust records a declared
    /// <c>Result::Err</c>. Any other exception is not part of the operation's
    /// contract: it becomes an unexpected target fault, which carries no CLR type
    /// or message, because Rust's fault channel records neither and a channel
    /// only one producer can fill is not comparable evidence.
    /// </remarks>
    internal void CompleteThrown(Exception exception)
    {
        if (!TryBeginCompletion())
        {
            return;
        }

        bool declared = OperationMetadata.For(Method).MapsException(exception);
        Func<ulong, long, Completion> completion = declared
            ? (order, time) => new ErrorCompletion(
                order, time, OperationMetadata.ErrorName, Value.String(exception.Message))
            : (order, time) => new FaultCompletion(
                order,
                time,
                NativeFault.UnexpectedTargetFault,
                NativeFault.UnwoundMessage,
                NativeFault.TargetObserver);
        Session!.Complete(Pending!, completion);
    }

    /// <summary>
    /// Completes the span for an operation its caller discarded before it
    /// reached an outcome. The span stays <c>UNSET</c> and no failure propagates
    /// to its parents, per <c>docs/decisions/abandonment-terminal-state.md</c>.
    /// </summary>
    internal void CompleteAbandoned()
    {
        if (!TryBeginCompletion())
        {
            return;
        }

        Session!.Complete(Pending!, (order, time) => new AbandonedCompletion(order, time));
    }

    private bool TryBeginCompletion()
    {
        if (Session is null || Pending is null || Completed)
        {
            return false;
        }

        Completed = true;
        return true;
    }
}
