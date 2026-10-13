using System.Collections.Generic;

namespace SpecGate.Runtime;

/// <summary>The terminal status of a recorded operation.</summary>
public enum SpanStatus
{
    /// <summary>The operation returned normally.</summary>
    Ok,

    /// <summary>The operation terminated by throwing.</summary>
    Error,
}

/// <summary>A named value observed at an operation boundary.</summary>
/// <remarks>
/// The value is the CTSC projection taken while the call was still on the stack,
/// not the original object. Keeping the object and converting it later would
/// record whatever it was mutated into after the operation returned.
/// </remarks>
public sealed class RecordedValue
{
    internal RecordedValue(string name, Value value)
    {
        Name = name;
        Value = value;
    }

    /// <summary>Gets the semantic value name.</summary>
    public string Name { get; }

    /// <summary>Gets the observed value.</summary>
    public Value Value { get; }
}

/// <summary>A completed operation observation produced by woven instrumentation.</summary>
public sealed class RecordedSpan
{
    internal RecordedSpan(
        long spanId,
        long? parentSpanId,
        string component,
        string operation,
        IReadOnlyList<RecordedValue> inputs,
        SpanStatus status,
        RecordedValue? result,
        string? errorType,
        string? errorMessage)
    {
        SpanId = spanId;
        ParentSpanId = parentSpanId;
        Component = component;
        Operation = operation;
        Inputs = inputs;
        Status = status;
        Result = result;
        ErrorType = errorType;
        ErrorMessage = errorMessage;
    }

    /// <summary>Gets the session-unique identifier of this span.</summary>
    public long SpanId { get; }

    /// <summary>Gets the enclosing operation's span identifier, when nested.</summary>
    public long? ParentSpanId { get; }

    /// <summary>Gets the owning component identifier.</summary>
    public string Component { get; }

    /// <summary>Gets the semantic operation name.</summary>
    public string Operation { get; }

    /// <summary>Gets the recorded inputs in declaration order.</summary>
    public IReadOnlyList<RecordedValue> Inputs { get; }

    /// <summary>Gets the terminal status.</summary>
    public SpanStatus Status { get; }

    /// <summary>Gets the recorded return value, absent for void and error terminations.</summary>
    public RecordedValue? Result { get; }

    /// <summary>Gets the thrown exception's type name when <see cref="Status"/> is error.</summary>
    public string? ErrorType { get; }

    /// <summary>Gets the thrown exception's message when <see cref="Status"/> is error.</summary>
    public string? ErrorMessage { get; }
}
