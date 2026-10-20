using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;

namespace SpecGate.Runtime;

/// <summary>The outcome a span reports.</summary>
/// <remarks>
/// This mirrors Rust's <c>Status</c>
/// (<c>rust/crates/specgate-runtime/src/capture/model.rs</c>).
/// </remarks>
public enum SpanStatus
{
    /// <summary>No outcome was reported, such as an abandoned operation.</summary>
    Unset,

    /// <summary>The operation reached its return boundary.</summary>
    Ok,

    /// <summary>The operation reported a declared error or a fault.</summary>
    Error,
}

/// <summary>A named semantic value observed while an operation ran.</summary>
public sealed class Observation
{
    /// <summary>Creates an observation.</summary>
    /// <param name="order">The stable event order within the capture.</param>
    /// <param name="timeUnixNano">The event timestamp in Unix nanoseconds.</param>
    /// <param name="name">The semantic observation name.</param>
    /// <param name="value">The semantic observation value.</param>
    public Observation(ulong order, long timeUnixNano, string name, Value value)
    {
        Order = order;
        TimeUnixNano = timeUnixNano;
        Name = name ?? throw new ArgumentNullException(nameof(name));
        ObservedValue = value ?? throw new ArgumentNullException(nameof(value));
    }

    /// <summary>Gets the stable event order within the capture.</summary>
    public ulong Order { get; }

    /// <summary>Gets the event timestamp in Unix nanoseconds.</summary>
    public long TimeUnixNano { get; }

    /// <summary>Gets the semantic observation name.</summary>
    public string Name { get; }

    /// <summary>Gets the semantic observation value.</summary>
    public Value ObservedValue { get; }
}

/// <summary>
/// The semantic completion recorded at an operation's actual return boundary.
/// </summary>
/// <remarks>
/// <para>
/// This is a closed set of terminal channels, mirroring Rust's
/// <c>Completion</c> (<c>rust/crates/specgate-runtime/src/capture/model.rs</c>):
/// a typed result, an empty successful absence, a declared error, an unmapped
/// fault, or an abandonment. A unit-returning operation reports no completion at
/// all, because it has no value channel to report on.
/// </para>
/// <para>
/// The hierarchy is sealed by a private protected constructor. A producer that
/// could invent a sixth channel could write a trace no consumer knows how to
/// compare.
/// </para>
/// </remarks>
public abstract class Completion
{
    private protected Completion(ulong order, long timeUnixNano)
    {
        Order = order;
        TimeUnixNano = timeUnixNano;
    }

    /// <summary>Gets the stable event order within the capture.</summary>
    public ulong Order { get; }

    /// <summary>Gets the completion timestamp in Unix nanoseconds.</summary>
    public long TimeUnixNano { get; }

    /// <summary>Gets the stable wire discriminator for this channel.</summary>
    public abstract string Kind { get; }

    /// <summary>Gets the span status this channel requires.</summary>
    public abstract SpanStatus RequiredStatus { get; }
}

/// <summary>A typed result completion.</summary>
public sealed class ResultCompletion : Completion
{
    /// <summary>Creates a result completion.</summary>
    /// <param name="order">The stable event order.</param>
    /// <param name="timeUnixNano">The completion timestamp in Unix nanoseconds.</param>
    /// <param name="value">The semantic result value.</param>
    public ResultCompletion(ulong order, long timeUnixNano, Value value)
        : base(order, timeUnixNano) =>
        ResultValue = value ?? throw new ArgumentNullException(nameof(value));

    /// <summary>Gets the semantic result value.</summary>
    public Value ResultValue { get; }

    /// <inheritdoc/>
    public override string Kind => "result";

    /// <inheritdoc/>
    public override SpanStatus RequiredStatus => SpanStatus.Ok;
}

/// <summary>A successful absence from an operation that has a value channel.</summary>
public sealed class EmptyCompletion : Completion
{
    /// <summary>Creates an empty completion.</summary>
    /// <param name="order">The stable event order.</param>
    /// <param name="timeUnixNano">The completion timestamp in Unix nanoseconds.</param>
    public EmptyCompletion(ulong order, long timeUnixNano)
        : base(order, timeUnixNano)
    {
    }

    /// <inheritdoc/>
    public override string Kind => "empty";

    /// <inheritdoc/>
    public override SpanStatus RequiredStatus => SpanStatus.Ok;
}

/// <summary>A declared error named by the registry.</summary>
public sealed class ErrorCompletion : Completion
{
    /// <summary>Creates a declared error completion.</summary>
    /// <param name="order">The stable event order.</param>
    /// <param name="timeUnixNano">The completion timestamp in Unix nanoseconds.</param>
    /// <param name="name">The registry error name.</param>
    /// <param name="value">The error payload, or <see langword="null"/> when the error channel carries no value.</param>
    public ErrorCompletion(ulong order, long timeUnixNano, string name, Value? value)
        : base(order, timeUnixNano)
    {
        Name = name ?? throw new ArgumentNullException(nameof(name));
        ErrorValue = value;
    }

    /// <summary>Gets the registry error name.</summary>
    public string Name { get; }

    /// <summary>Gets the error payload, or <see langword="null"/> when there is none.</summary>
    public Value? ErrorValue { get; }

    /// <inheritdoc/>
    public override string Kind => "error";

    /// <inheritdoc/>
    public override SpanStatus RequiredStatus => SpanStatus.Error;
}

/// <summary>An unmapped failure observed at the target.</summary>
public sealed class FaultCompletion : Completion
{
    /// <summary>Creates a fault completion.</summary>
    /// <param name="order">The stable event order.</param>
    /// <param name="timeUnixNano">The completion timestamp in Unix nanoseconds.</param>
    /// <param name="faultType">The stable fault type identifier.</param>
    /// <param name="message">The human-readable fault description.</param>
    /// <param name="observer">The stable identifier of who observed the fault.</param>
    public FaultCompletion(ulong order, long timeUnixNano, string faultType, string message, string observer)
        : base(order, timeUnixNano)
    {
        FaultType = faultType ?? throw new ArgumentNullException(nameof(faultType));
        Message = message ?? throw new ArgumentNullException(nameof(message));
        Observer = observer ?? throw new ArgumentNullException(nameof(observer));
    }

    /// <summary>Gets the stable fault type identifier.</summary>
    public string FaultType { get; }

    /// <summary>Gets the human-readable fault description.</summary>
    public string Message { get; }

    /// <summary>Gets the stable identifier of who observed the fault.</summary>
    public string Observer { get; }

    /// <inheritdoc/>
    public override string Kind => "fault";

    /// <inheritdoc/>
    public override SpanStatus RequiredStatus => SpanStatus.Error;
}

/// <summary>An operation that never reached a terminal channel.</summary>
public sealed class AbandonedCompletion : Completion
{
    /// <summary>Creates an abandoned completion.</summary>
    /// <param name="order">The stable event order.</param>
    /// <param name="timeUnixNano">The completion timestamp in Unix nanoseconds.</param>
    public AbandonedCompletion(ulong order, long timeUnixNano)
        : base(order, timeUnixNano)
    {
    }

    /// <inheritdoc/>
    public override string Kind => "abandoned";

    /// <inheritdoc/>
    public override SpanStatus RequiredStatus => SpanStatus.Unset;
}

/// <summary>One recorded annotated operation.</summary>
public sealed class OperationSpan
{
    /// <summary>Creates an operation span.</summary>
    /// <param name="order">The stable start order within the capture.</param>
    /// <param name="spanId">This span's identifier.</param>
    /// <param name="parentSpanId">The enclosing span's identifier.</param>
    /// <param name="componentId">The dotted component name.</param>
    /// <param name="operationName">The declared operation name.</param>
    /// <param name="startTimeUnixNano">The span start timestamp in Unix nanoseconds.</param>
    /// <param name="endTimeUnixNano">The span end timestamp in Unix nanoseconds.</param>
    /// <param name="status">The reported outcome.</param>
    /// <param name="inputs">The recorded inputs, keyed by semantic name.</param>
    /// <param name="observations">Observations recorded while the operation ran.</param>
    /// <param name="completion">The terminal channel, or <see langword="null"/> for a unit operation.</param>
    /// <exception cref="ArgumentException">
    /// The span ends before it starts, or <paramref name="status"/> contradicts
    /// <paramref name="completion"/>.
    /// </exception>
    public OperationSpan(
        ulong order,
        SpanId spanId,
        SpanId parentSpanId,
        string componentId,
        string operationName,
        long startTimeUnixNano,
        long endTimeUnixNano,
        SpanStatus status,
        IReadOnlyDictionary<string, Value> inputs,
        IReadOnlyList<Observation> observations,
        Completion? completion)
    {
        if (endTimeUnixNano < startTimeUnixNano)
        {
            throw new ArgumentException(
                $"specgate: operation '{operationName}' ends at {endTimeUnixNano} before it starts at {startTimeUnixNano}.");
        }

        // A unit operation has no value channel, so it reports no completion and
        // is unconditionally OK. Every other channel pins the status exactly.
        SpanStatus required = completion?.RequiredStatus ?? SpanStatus.Ok;
        if (status != required)
        {
            throw new ArgumentException(
                $"specgate: operation '{operationName}' reports status {status} with "
                + $"{completion?.Kind ?? "no"} completion, which requires {required}.");
        }

        Order = order;
        SpanId = spanId ?? throw new ArgumentNullException(nameof(spanId));
        ParentSpanId = parentSpanId ?? throw new ArgumentNullException(nameof(parentSpanId));
        ComponentId = componentId ?? throw new ArgumentNullException(nameof(componentId));
        OperationName = operationName ?? throw new ArgumentNullException(nameof(operationName));
        StartTimeUnixNano = startTimeUnixNano;
        EndTimeUnixNano = endTimeUnixNano;
        Status = status;
        ArgumentNullException.ThrowIfNull(inputs);

        // The wire form is a sorted map, so the order is fixed here rather than
        // trusting whatever order the caller's dictionary enumerates in.
        SortedDictionary<string, Value> ordered = new(Utf8KeyComparer.Instance);
        foreach (KeyValuePair<string, Value> input in inputs)
        {
            ordered.Add(input.Key, input.Value);
        }

        Inputs = new ReadOnlyDictionary<string, Value>(ordered);
        Observations = observations ?? throw new ArgumentNullException(nameof(observations));
        OperationCompletion = completion;
    }

    /// <summary>Gets the stable start order within the capture.</summary>
    public ulong Order { get; }

    /// <summary>Gets this span's identifier.</summary>
    public SpanId SpanId { get; }

    /// <summary>Gets the enclosing span's identifier.</summary>
    public SpanId ParentSpanId { get; }

    /// <summary>Gets the dotted component name.</summary>
    public string ComponentId { get; }

    /// <summary>Gets the declared operation name.</summary>
    public string OperationName { get; }

    /// <summary>Gets the span start timestamp in Unix nanoseconds.</summary>
    public long StartTimeUnixNano { get; }

    /// <summary>Gets the span end timestamp in Unix nanoseconds.</summary>
    public long EndTimeUnixNano { get; }

    /// <summary>Gets the reported outcome.</summary>
    public SpanStatus Status { get; }

    /// <summary>Gets the recorded inputs, keyed by semantic name.</summary>
    public IReadOnlyDictionary<string, Value> Inputs { get; }

    /// <summary>Gets the observations recorded while the operation ran.</summary>
    public IReadOnlyList<Observation> Observations { get; }

    /// <summary>Gets the terminal channel, or <see langword="null"/> for a unit operation.</summary>
    public Completion? OperationCompletion { get; }
}

/// <summary>A run or scenario span that encloses the recorded operations.</summary>
public sealed class SpanBoundary
{
    /// <summary>Creates a boundary span.</summary>
    /// <param name="spanId">This span's identifier.</param>
    /// <param name="parentSpanId">The enclosing span's identifier, or <see langword="null"/> for the root.</param>
    /// <param name="startTimeUnixNano">The span start timestamp in Unix nanoseconds.</param>
    /// <param name="endTimeUnixNano">The span end timestamp in Unix nanoseconds.</param>
    /// <param name="status">The reported outcome.</param>
    public SpanBoundary(SpanId spanId, SpanId? parentSpanId, long startTimeUnixNano, long endTimeUnixNano, SpanStatus status)
    {
        if (endTimeUnixNano < startTimeUnixNano)
        {
            throw new ArgumentException(
                $"specgate: boundary span '{spanId}' ends at {endTimeUnixNano} before it starts at {startTimeUnixNano}.");
        }

        SpanId = spanId ?? throw new ArgumentNullException(nameof(spanId));
        ParentSpanId = parentSpanId;
        StartTimeUnixNano = startTimeUnixNano;
        EndTimeUnixNano = endTimeUnixNano;
        Status = status;
    }

    /// <summary>Gets this span's identifier.</summary>
    public SpanId SpanId { get; }

    /// <summary>Gets the enclosing span's identifier, or <see langword="null"/> for the root.</summary>
    public SpanId? ParentSpanId { get; }

    /// <summary>Gets the span start timestamp in Unix nanoseconds.</summary>
    public long StartTimeUnixNano { get; }

    /// <summary>Gets the span end timestamp in Unix nanoseconds.</summary>
    public long EndTimeUnixNano { get; }

    /// <summary>Gets the reported outcome.</summary>
    public SpanStatus Status { get; }
}

/// <summary>One finished capture: the run and scenario spans and every operation.</summary>
public sealed class Capture
{
    /// <summary>Creates a capture.</summary>
    /// <param name="traceId">The trace identifier shared by every span.</param>
    /// <param name="scenarioName">The stable scenario name.</param>
    /// <param name="run">The run boundary span.</param>
    /// <param name="scenario">The scenario boundary span.</param>
    /// <param name="operations">The operation spans, in start order.</param>
    public Capture(
        TraceId traceId, string scenarioName, SpanBoundary run, SpanBoundary scenario, IReadOnlyList<OperationSpan> operations)
    {
        TraceId = traceId ?? throw new ArgumentNullException(nameof(traceId));
        ScenarioName = scenarioName ?? throw new ArgumentNullException(nameof(scenarioName));
        Run = run ?? throw new ArgumentNullException(nameof(run));
        Scenario = scenario ?? throw new ArgumentNullException(nameof(scenario));
        Operations = operations ?? throw new ArgumentNullException(nameof(operations));
    }

    /// <summary>Gets the trace identifier shared by every span.</summary>
    public TraceId TraceId { get; }

    /// <summary>Gets the stable scenario name.</summary>
    public string ScenarioName { get; }

    /// <summary>Gets the run boundary span.</summary>
    public SpanBoundary Run { get; }

    /// <summary>Gets the scenario boundary span.</summary>
    public SpanBoundary Scenario { get; }

    /// <summary>Gets the operation spans, in start order.</summary>
    public IReadOnlyList<OperationSpan> Operations { get; }

    /// <summary>An empty read-only input map.</summary>
    internal static readonly IReadOnlyDictionary<string, Value> NoInputs =
        new ReadOnlyDictionary<string, Value>(new SortedDictionary<string, Value>(Utf8KeyComparer.Instance));

    /// <summary>An empty read-only observation list.</summary>
    internal static readonly IReadOnlyList<Observation> NoObservations = Array.Empty<Observation>();
}
