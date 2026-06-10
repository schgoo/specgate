namespace SpecGate;

/// <summary>
/// Marks a field or property as a state snapshot within a spec operation.
/// Only valid for StateMachine kind.
/// </summary>
[AttributeUsage(AttributeTargets.Field | AttributeTargets.Property, AllowMultiple = true)]
public sealed class SpecStateAttribute : Attribute
{
    /// <summary>
    /// The operation name this state belongs to.
    /// </summary>
    public string Name { get; }

    public SpecStateAttribute(string name)
    {
        Name = name;
    }
}
