namespace SpecGate;

/// <summary>
/// Marks a property or method as an external dependency for a spec operation.
/// </summary>
[AttributeUsage(AttributeTargets.Property | AttributeTargets.Method, AllowMultiple = true)]
public sealed class SpecDependencyAttribute : Attribute
{
    /// <summary>
    /// The operation name this dependency belongs to.
    /// </summary>
    public string Name { get; }

    /// <summary>
    /// The dependency category (e.g., "external_service", "clock", "random").
    /// </summary>
    public string Dep { get; set; } = "";

    public SpecDependencyAttribute(string name)
    {
        Name = name;
    }
}
