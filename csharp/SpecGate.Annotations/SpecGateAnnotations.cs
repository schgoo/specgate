using System;

namespace SpecGate.Annotations;

/// <summary>
/// Marks a public method as the implementation of a SpecGate operation.
/// The harness discovers methods with this attribute and invokes the matching
/// operation name from a spec case when it builds a language-binding runner.
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = true)]
public sealed class SpecOperationAttribute : Attribute
{
    /// <summary>
    /// Gets the spec operation name implemented by the annotated method.
    /// </summary>
    public string Name { get; }

    /// <summary>
    /// Gets or sets the spec component name (e.g., spec.name).
    /// The harness resolves operations as (component, operation).
    /// </summary>
    public string? Spec { get; set; }

    /// <summary>
    /// Initializes a new instance of the <see cref="SpecOperationAttribute"/> class.
    /// </summary>
    /// <param name="name">The operation name as it appears in the spec.</param>
    public SpecOperationAttribute(string name) => Name = name;
}

/// <summary>
/// Marks a public setup method that constructs state or input objects before a
/// SpecGate operation is invoked.
/// </summary>
/// <remarks>
/// Setup methods let the harness reproduce stateful spec cases without requiring
/// fixture code to emit trace records manually. The harness captures annotated
/// event members on returned setup objects and wires them into operation calls.
/// </remarks>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = true)]
public sealed class SpecSetupAttribute : Attribute
{
    /// <summary>
    /// Gets the operation name whose case setup should use the annotated method.
    /// </summary>
    public string Name { get; }

    /// <summary>
    /// Gets or sets the spec component name (e.g., spec.name).
    /// When set, the setup is scoped to operations in that component.
    /// </summary>
    public string? Spec { get; set; }

    /// <summary>
    /// Gets or sets the operation parameter name filled by the setup result.
    /// When unset, the setup result may be used as the method receiver or as a
    /// type-matched parameter.
    /// </summary>
    public string? Fills { get; set; }

    /// <summary>
    /// Initializes a new instance of the <see cref="SpecSetupAttribute"/> class.
    /// </summary>
    /// <param name="name">The operation name whose cases use this setup.</param>
    public SpecSetupAttribute(string name) => Name = name;
}

/// <summary>
/// Assigns the spec input name for a method parameter.
/// </summary>
/// <remarks>
/// Use this attribute when the C# parameter identifier differs from the spec
/// input key, such as escaped keywords or idiomatic local naming.
/// </remarks>
[AttributeUsage(AttributeTargets.Parameter)]
public sealed class SpecInputAttribute : Attribute
{
    /// <summary>
    /// Gets the input name as declared by the spec.
    /// </summary>
    public string Name { get; }

    /// <summary>
    /// Initializes a new instance of the <see cref="SpecInputAttribute"/> class.
    /// </summary>
    /// <param name="name">The spec input key represented by the annotated parameter.</param>
    public SpecInputAttribute(string name) => Name = name;
}

/// <summary>
/// Opts a type or member into SpecGate event serialization.
/// </summary>
/// <remarks>
/// When placed on a public field or property, the runtime can emit that member
/// as a named trace event or as part of a structured result value. When placed on
/// a class or struct, the runtime treats instances as structured SpecGate values.
/// </remarks>
[AttributeUsage(AttributeTargets.Class | AttributeTargets.Struct | AttributeTargets.Field | AttributeTargets.Property)]
public sealed class SpecEventAttribute : Attribute
{
    /// <summary>
    /// Gets the trace event or structured field name override, or <see langword="null"/>
    /// when the member or type name should be used.
    /// </summary>
    public string? Name { get; }

    /// <summary>
    /// Initializes a new instance of the <see cref="SpecEventAttribute"/> class
    /// using the annotated symbol name as the event name.
    /// </summary>
    public SpecEventAttribute() { }

    /// <summary>
    /// Initializes a new instance of the <see cref="SpecEventAttribute"/> class
    /// with an explicit event or field name.
    /// </summary>
    /// <param name="name">The name to use in SpecGate traces.</param>
    public SpecEventAttribute(string name) => Name = name;
}

/// <summary>
/// Marks a dependency field as a table-driven mock. Calls made through the
/// annotated field inside a SpecGate operation are intercepted by the harness:
/// the real dependency is never invoked. Each intercepted call emits a
/// <c>&lt;name&gt;.request</c> event with its (last) argument, then either a
/// <c>&lt;name&gt;.response</c> event carrying the table value on a hit, or a
/// <c>&lt;name&gt;.error</c> event on a miss (in which case the operation
/// returns its default value).
/// </summary>
/// <remarks>
/// The mock table is seeded per case from a spec input map keyed by the mock
/// <see cref="Name"/>. The annotated field's type may be any concrete type,
/// including third-party or sealed types, because interception happens at the
/// call site rather than by substituting the type.
/// </remarks>
[AttributeUsage(AttributeTargets.Field)]
public sealed class SpecMockAttribute : Attribute
{
    /// <summary>
    /// Gets the mock name, used both as the spec-input key for the response
    /// table and as the prefix for the emitted <c>request</c>/<c>response</c>/
    /// <c>error</c> events.
    /// </summary>
    public string Name { get; }

    /// <summary>
    /// Initializes a new instance of the <see cref="SpecMockAttribute"/> class.
    /// </summary>
    /// <param name="name">The mock name (spec-input key and event prefix).</param>
    public SpecMockAttribute(string name) => Name = name;
}
