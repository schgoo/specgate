using System;

namespace SpecGate.Annotations;

/// <summary>Marks a method as a semantic CTSC operation.</summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = true)]
public sealed class SpecOperationAttribute : Attribute
{
    /// <summary>Initializes an operation annotation.</summary>
    public SpecOperationAttribute(string name) => Name = name;

    /// <summary>Gets the semantic operation name.</summary>
    public string Name { get; }

    /// <summary>Gets or sets the owning component identifier.</summary>
    public string? Spec { get; set; }
}

/// <summary>Marks a method as a setup producer for one component operation.</summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = true)]
public sealed class SpecSetupAttribute : Attribute
{
    /// <summary>Initializes a setup annotation.</summary>
    public SpecSetupAttribute(string name) => Name = name;

    /// <summary>Gets the operation prepared by this setup.</summary>
    public string Name { get; }

    /// <summary>Gets or sets the owning component identifier.</summary>
    public string? Spec { get; set; }

    /// <summary>Gets or sets the operation parameter filled by the setup result.</summary>
    public string? Fills { get; set; }
}

/// <summary>Assigns a language-neutral semantic name to a parameter.</summary>
[AttributeUsage(AttributeTargets.Parameter)]
public sealed class SpecInputAttribute : Attribute
{
    /// <summary>Initializes an input annotation.</summary>
    public SpecInputAttribute(string name) => Name = name;

    /// <summary>Gets the semantic input name.</summary>
    public string Name { get; }
}

/// <summary>Marks a type or member for semantic type discovery.</summary>
[AttributeUsage(AttributeTargets.Class | AttributeTargets.Struct | AttributeTargets.Field | AttributeTargets.Property)]
public sealed class SpecEventAttribute : Attribute
{
    /// <summary>Initializes an annotation that uses the symbol's own name.</summary>
    public SpecEventAttribute() { }

    /// <summary>Initializes an annotation with an explicit semantic name.</summary>
    public SpecEventAttribute(string name) => Name = name;

    /// <summary>Gets the optional semantic name override.</summary>
    public string? Name { get; }
}

/// <summary>Declares exception types mapped to a semantic error outcome.</summary>
[AttributeUsage(AttributeTargets.Method)]
public sealed class SpecExceptionAttribute : Attribute
{
    /// <summary>Initializes an exception mapping; no types means catch-all.</summary>
    public SpecExceptionAttribute(params Type[] exceptionTypes) => ExceptionTypes = exceptionTypes;

    /// <summary>Gets the mapped exception types.</summary>
    public Type[] ExceptionTypes { get; }
}

/// <summary>A language-neutral optional value used by C# metadata discovery.</summary>
public readonly struct Option<T>
{
    private Option(T? value, bool hasValue)
    {
        Value = value;
        HasValue = hasValue;
    }

    /// <summary>Gets whether a value is present.</summary>
    public bool HasValue { get; }

    /// <summary>Gets the optional value.</summary>
    public T? Value { get; }

    /// <summary>Creates a present value.</summary>
    public static Option<T> Some(T value) => new(value, true);

    /// <summary>Creates an absent value.</summary>
    public static Option<T> None() => new(default, false);
}

/// <summary>A language-neutral result value used by C# metadata discovery.</summary>
public readonly struct Result<T, E>
{
    private Result(T? ok, E? error, bool isOk)
    {
        OkValue = ok;
        ErrValue = error;
        IsOk = isOk;
    }

    /// <summary>Gets whether this value is successful.</summary>
    public bool IsOk { get; }

    /// <summary>Gets the success value.</summary>
    public T? OkValue { get; }

    /// <summary>Gets the error value.</summary>
    public E? ErrValue { get; }

    /// <summary>Creates a successful result.</summary>
    public static Result<T, E> Ok(T value) => new(value, default, true);

    /// <summary>Creates an error result.</summary>
    public static Result<T, E> Err(E error) => new(default, error, false);
}
