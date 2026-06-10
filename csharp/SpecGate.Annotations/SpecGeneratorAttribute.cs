using System;

namespace SpecGate;

/// <summary>
/// Marks a method as a factory for constructing instances of a type that has
/// no public constructor or decomposable fields.
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = false)]
public sealed class SpecGeneratorAttribute : Attribute
{
    /// <summary>
    /// The fully qualified type name this generator constructs.
    /// </summary>
    public string TypeName { get; }

    public SpecGeneratorAttribute(string typeName)
    {
        TypeName = typeName;
    }
}
