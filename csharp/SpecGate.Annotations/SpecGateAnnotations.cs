using System;

namespace SpecGate.Annotations;

[AttributeUsage(AttributeTargets.Method)]
public sealed class SpecOperationAttribute : Attribute
{
    public string Name { get; }
    public SpecOperationAttribute(string name) => Name = name;
}

[AttributeUsage(AttributeTargets.Parameter)]
public sealed class SpecInputAttribute : Attribute
{
    public string Name { get; }
    public SpecInputAttribute(string name) => Name = name;
}
