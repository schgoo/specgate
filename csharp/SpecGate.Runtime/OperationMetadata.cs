using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Threading.Tasks;
using SpecGate.Annotations;

namespace SpecGate.Runtime;

/// <summary>
/// The CTSC meaning of one woven method's inputs and return value.
/// </summary>
/// <remarks>
/// <para>
/// Woven code passes the method's own handle rather than a description of its
/// signature, and everything recorded about the shape of the operation is read
/// back from reflection here. That keeps one interpretation of C# declarations:
/// the weaver decides where a boundary is, and this runtime decides what the
/// values at that boundary mean.
/// </para>
/// <para>
/// Metadata is resolved on first recorded use and cached, so a process with no
/// capture session attached never reflects at all.
/// </para>
/// </remarks>
internal sealed class OperationMetadata
{
    /// <summary>The name recorded for an operation's return value.</summary>
    internal const string ResultName = "return";

    /// <summary>
    /// The name recorded for an operation's declared error outcome.
    /// </summary>
    /// <remarks>
    /// Rust's annotation macro names every <c>Result::Err</c> channel <c>error</c>
    /// (<c>rust/crates/specgate-annotations-macros-impl/src/lib.rs:19</c>), and
    /// C# discovery publishes a <c>[SpecException]</c> method as the same single
    /// error channel, so both producers record one name.
    /// </remarks>
    internal const string ErrorName = "error";

    private static readonly ConcurrentDictionary<RuntimeMethodHandle, OperationMetadata> Cache = new();
    private static readonly ConcurrentDictionary<(RuntimeMethodHandle Method, string Component, string Operation), string> OperationNames = new();

    private OperationMetadata(SemanticInput[] inputs, SemanticType? result, Type[]? declaredExceptions)
    {
        Inputs = inputs;
        Result = result;
        DeclaredExceptions = declaredExceptions;
    }

    internal SemanticInput[] Inputs { get; }

    /// <summary>The return value's meaning, or <see langword="null"/> for a void operation.</summary>
    internal SemanticType? Result { get; }

    /// <summary>
    /// The exception types the operation maps to its declared error channel,
    /// or <see langword="null"/> when it declares no error channel at all. An
    /// empty array means the declaration listed no types, which
    /// <see cref="SpecExceptionAttribute"/> defines as catch-all.
    /// </summary>
    internal Type[]? DeclaredExceptions { get; }

    /// <summary>
    /// Whether <paramref name="exception"/> belongs to the operation's declared
    /// error channel rather than being an unexpected target fault.
    /// </summary>
    internal bool MapsException(Exception exception)
    {
        if (DeclaredExceptions is null)
        {
            return false;
        }

        if (DeclaredExceptions.Length == 0)
        {
            return true;
        }

        foreach (Type declared in DeclaredExceptions)
        {
            if (declared.IsInstanceOfType(exception))
            {
                return true;
            }
        }

        return false;
    }

    internal static OperationMetadata For(RuntimeMethodHandle handle) => Cache.GetOrAdd(handle, Describe);

    internal static string OperationName(RuntimeMethodHandle method, string component, string operation) =>
        OperationNames.GetOrAdd((method, component, operation), static key =>
        {
            if (MethodBase.GetMethodFromHandle(key.Method) is not MethodInfo { IsGenericMethod: true } closed)
            {
                return key.Operation;
            }

            Type[] arguments = closed.GetGenericArguments();
            SpecOperationAttribute[] matches = [.. closed.GetGenericMethodDefinition()
                .GetCustomAttributes<SpecOperationAttribute>()
                .Where(annotation => annotation.Spec == key.Component && annotation.TypeArguments.SequenceEqual(arguments))];
            if (matches.Length != 1)
            {
                throw new SpecGateCaptureException(
                    CaptureRejection.UnsupportedType,
                    $"specgate: generic operation '{closed}' requires exactly one declared specialization for these type arguments.");
            }

            return matches[0].Name;
        });

    private static OperationMetadata Describe(RuntimeMethodHandle handle)
    {
        MethodBase method = MethodBase.GetMethodFromHandle(handle)
            ?? throw new SpecGateCaptureException(
                CaptureRejection.UnsupportedType,
                "specgate: a woven operation's method could not be resolved.");

        ParameterInfo[] parameters = method.GetParameters();
        SemanticInput[] inputs = new SemanticInput[parameters.Length];
        for (int index = 0; index < parameters.Length; index++)
        {
            ParameterInfo parameter = parameters[index];
            inputs[index] = new SemanticInput(
                InputName(parameter), SemanticType.ForParameter(parameter, method));
        }

        RejectDuplicates(method, inputs);

        SemanticType? result = null;
        if (method is MethodInfo function && function.ReturnType != typeof(void))
        {
            result = ResultOf(function);
        }

        return new OperationMetadata(inputs, result, DeclaredExceptionsOf(method));
    }

    /// <summary>
    /// The meaning of what an operation produces, which for an asynchronous
    /// operation is the awaited value rather than the task that carries it.
    /// </summary>
    /// <remarks>
    /// This mirrors discovery's unwrapping, so a captured result has the type the
    /// registry publishes. The task's own nullability byte precedes its type
    /// argument's, and is consumed before the awaited type is mapped.
    /// </remarks>
    private static SemanticType? ResultOf(MethodInfo function)
    {
        Type returnType = function.ReturnType;
        if (returnType == typeof(Task) || returnType == typeof(ValueTask))
        {
            return null;
        }

        if (returnType.IsGenericType
            && (returnType.GetGenericTypeDefinition() == typeof(Task<>)
                || returnType.GetGenericTypeDefinition() == typeof(ValueTask<>)))
        {
            NullableFlagReader flags = NullableFlagReader.ForParameter(function.ReturnParameter, function);
            flags.Next();
            return SemanticType.Map(returnType.GetGenericArguments()[0], flags);
        }

        return SemanticType.ForParameter(function.ReturnParameter, function);
    }

    /// <remarks>
    /// Inheritance is honored so an override that does not repeat
    /// <c>[SpecException]</c> keeps the base declaration's error channel, which
    /// is what the discovered registry publishes for it.
    /// </remarks>
    private static Type[]? DeclaredExceptionsOf(MethodBase method)
    {
        SpecExceptionAttribute? annotation = method.GetCustomAttribute<SpecExceptionAttribute>(inherit: true);
        if (annotation is null)
        {
            return null;
        }

        return annotation.ExceptionTypes ?? [];
    }

    /// <remarks>
    /// Any declared name is preserved exactly, including an empty one, because
    /// C# discovery publishes <c>inp?.Name ?? p.Name</c>. Treating an empty
    /// declared name as absent would make capture and the discovered registry
    /// disagree about the same parameter.
    /// </remarks>
    private static string InputName(ParameterInfo parameter)
    {
        SpecInputAttribute? annotation = parameter.GetCustomAttribute<SpecInputAttribute>(inherit: false);
        return annotation?.Name ?? parameter.Name!;
    }

    private static void RejectDuplicates(MethodBase method, SemanticInput[] inputs)
    {
        HashSet<string> seen = new(StringComparer.Ordinal);
        foreach (SemanticInput input in inputs)
        {
            if (!seen.Add(input.Name))
            {
                throw new SpecGateCaptureException(
                    CaptureRejection.AmbiguousDeclaration,
                    $"specgate: '{method.DeclaringType?.FullName}.{method.Name}' records the input "
                    + $"name '{input.Name}' more than once.");
            }
        }
    }
}

/// <summary>One recorded operation input.</summary>
internal sealed class SemanticInput(string name, SemanticType type)
{
    internal string Name { get; } = name;

    internal SemanticType Type { get; } = type;
}
