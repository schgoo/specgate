namespace SpecGate;

/// <summary>
/// Determines the extraction strategy and which annotation roles are valid.
/// </summary>
public enum SpecKind
{
    /// <summary>
    /// Pure function: same inputs always produce the same output.
    /// </summary>
    Pure,

    /// <summary>
    /// State machine: transitions between states with invariants.
    /// </summary>
    StateMachine,

    /// <summary>
    /// Ordered sequence of steps that must execute in order.
    /// </summary>
    Sequence,

    /// <summary>
    /// Maps error conditions to specific error types or codes.
    /// </summary>
    ErrorMap,

    /// <summary>
    /// Structural constraints: dependency rules, topology, type assertions.
    /// </summary>
    Structural
}
