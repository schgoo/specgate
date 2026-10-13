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
    /// Projects and records every argument the woven call received, in parameter
    /// order.
    /// </summary>
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

        RecordedValue[] recorded = new RecordedValue[declared.Length];
        for (int index = 0; index < declared.Length; index++)
        {
            recorded[index] = new RecordedValue(
                declared[index].Name, ValueProjector.Project(declared[index].Type, arguments[index]));
        }

        lock (_inputs)
        {
            _inputs.AddRange(recorded);
        }
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
