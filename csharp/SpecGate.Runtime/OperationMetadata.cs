using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Reflection;
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

    private static readonly ConcurrentDictionary<RuntimeMethodHandle, OperationMetadata> Cache = new();

    private OperationMetadata(SemanticInput[] inputs, SemanticType? result)
    {
        Inputs = inputs;
        Result = result;
    }

    internal SemanticInput[] Inputs { get; }

    /// <summary>The return value's meaning, or <see langword="null"/> for a void operation.</summary>
    internal SemanticType? Result { get; }

    internal static OperationMetadata For(RuntimeMethodHandle handle) => Cache.GetOrAdd(handle, Describe);

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
            result = SemanticType.ForParameter(function.ReturnParameter, function);
        }

        return new OperationMetadata(inputs, result);
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
