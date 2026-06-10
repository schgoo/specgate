namespace SpecGate;

/// <summary>
/// Marks a method as the entry point for a spec operation.
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = true)]
public sealed class SpecOperationAttribute : Attribute
{
    /// <summary>
    /// The operation name that groups all annotations for this operation.
    /// </summary>
    public string Name { get; }

    /// <summary>
    /// The kind of operation, which determines extraction strategy.
    /// </summary>
    public SpecKind Kind { get; }

    public SpecOperationAttribute(string name, SpecKind kind)
    {
        Name = name;
        Kind = kind;
    }
}
