using System;

namespace SpecGate;

/// <summary>
/// Marks a constructor, property, or setter as providing an input to a spec operation.
/// </summary>
[AttributeUsage(
    AttributeTargets.Constructor | AttributeTargets.Property | AttributeTargets.Method,
    AllowMultiple = true)]
public sealed class SpecInputAttribute : Attribute
{
    /// <summary>
    /// The operation name this input belongs to.
    /// </summary>
    public string Name { get; }

    public SpecInputAttribute(string name)
    {
        Name = name;
    }
}
