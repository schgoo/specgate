using System;

namespace SpecGate;

/// <summary>
/// Marks a method as an observable checkpoint within a spec operation.
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = true)]
public sealed class SpecCheckpointAttribute : Attribute
{
    /// <summary>
    /// The operation name this checkpoint belongs to.
    /// </summary>
    public string Name { get; }

    public SpecCheckpointAttribute(string name)
    {
        Name = name;
    }
}
