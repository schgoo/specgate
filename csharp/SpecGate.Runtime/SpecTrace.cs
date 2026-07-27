namespace SpecGate.Runtime;

/// <summary>
/// Provides fixture-facing trace checkpoint emission.
/// </summary>
public static class SpecTrace
{
    /// <summary>
    /// Emits a named string-valued checkpoint event into the current trace.
    /// </summary>
    /// <param name="name">The checkpoint event name expected by the spec.</param>
    /// <param name="value">The string value to serialize.</param>
    public static void Emit(string name, string value) => SpecGateRuntime.EmitEvent(name, value);

    /// <summary>
    /// Emits a named integer-valued checkpoint event into the current trace.
    /// </summary>
    /// <param name="name">The checkpoint event name expected by the spec.</param>
    /// <param name="value">The integer value to serialize.</param>
    public static void Emit(string name, int value) => SpecGateRuntime.EmitEvent(name, value);

    /// <summary>
    /// Emits a named Boolean-valued checkpoint event into the current trace.
    /// </summary>
    /// <param name="name">The checkpoint event name expected by the spec.</param>
    /// <param name="value">The Boolean value to serialize.</param>
    public static void Emit(string name, bool value) => SpecGateRuntime.EmitEvent(name, value);

    /// <summary>
    /// Emits a named checkpoint event for an arbitrary value into the current trace.
    /// </summary>
    /// <param name="name">The checkpoint event name expected by the spec.</param>
    /// <param name="value">The value to serialize using SpecGate's deterministic trace conversion.</param>
    public static void Emit(string name, object? value) => SpecGateRuntime.EmitEvent(name, value);
}
