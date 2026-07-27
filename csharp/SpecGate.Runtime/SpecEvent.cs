namespace SpecGate.Runtime;

/// <summary>
/// Fixture-facing API for recording an inline trace event mid-operation — the
/// C# analog of Rust's <c>spec_trace!</c> macro. Shares the "event" vocabulary
/// with the <c>[SpecEvent]</c> property attribute: the attribute marks a member
/// as an emitted event, while <see cref="Record(string, object?)"/> records one
/// inline. Fixtures use this instead of the runner-internal
/// <c>SpecGateRuntime</c> emit primitives.
/// </summary>
public static class SpecEvent
{
    /// <summary>Records a named string-valued event into the current trace.</summary>
    /// <param name="name">The event name expected by the spec.</param>
    /// <param name="value">The string value to serialize.</param>
    public static void Record(string name, string value) => SpecGateRuntime.EmitEvent(name, value);

    /// <summary>Records a named integer-valued event into the current trace.</summary>
    /// <param name="name">The event name expected by the spec.</param>
    /// <param name="value">The integer value to serialize.</param>
    public static void Record(string name, int value) => SpecGateRuntime.EmitEvent(name, value);

    /// <summary>Records a named Boolean-valued event into the current trace.</summary>
    /// <param name="name">The event name expected by the spec.</param>
    /// <param name="value">The Boolean value to serialize.</param>
    public static void Record(string name, bool value) => SpecGateRuntime.EmitEvent(name, value);

    /// <summary>Records a named event for an arbitrary value into the current trace.</summary>
    /// <param name="name">The event name expected by the spec.</param>
    /// <param name="value">The value to serialize using SpecGate's deterministic trace conversion.</param>
    public static void Record(string name, object? value) => SpecGateRuntime.EmitEvent(name, value);
}
