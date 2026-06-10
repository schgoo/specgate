using System;

namespace SpecGate;

/// <summary>
/// Marks a property or method as reading ambient state (environment) for a spec operation.
/// </summary>
[AttributeUsage(AttributeTargets.Property | AttributeTargets.Method, AllowMultiple = true)]
public sealed class SpecEnvironmentAttribute : Attribute
{
    /// <summary>
    /// The operation name this environment belongs to.
    /// </summary>
    public string Name { get; }

    public SpecEnvironmentAttribute(string name)
    {
        Name = name;
    }
}
