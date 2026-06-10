using System;

namespace SpecGate;

/// <summary>
/// Marks a static method as a context provider that wires spec primitives
/// into ambient state mechanisms. The method must be static, return void,
/// and be callable from the test project.
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = false)]
public sealed class SpecContextAttribute : Attribute
{
    /// <summary>
    /// The context name, matching the spec's environment or context field.
    /// </summary>
    public string Name { get; }

    public SpecContextAttribute(string name)
    {
        Name = name;
    }
}
