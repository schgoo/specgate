using System;
using System.Collections.Generic;
using System.Linq;
using Mono.Cecil;

namespace SpecGate.Weaver;

/// <summary>
/// The constraints every SpecGate-annotated method must satisfy to be woven,
/// shared by operations and setup producers.
/// </summary>
internal static class AnnotatedMethod
{
    internal const string SpecOperationAttribute = "SpecGate.Annotations.SpecOperationAttribute";
    internal const string SpecSetupAttribute = "SpecGate.Annotations.SpecSetupAttribute";

    private const string AsyncStateMachineAttribute =
        "System.Runtime.CompilerServices.AsyncStateMachineAttribute";
    private const string IteratorStateMachineAttribute =
        "System.Runtime.CompilerServices.IteratorStateMachineAttribute";

    internal static List<CustomAttribute> Annotations(MethodDefinition method, string attributeFullName) =>
    [
        .. method.CustomAttributes.Where(attribute => string.Equals(
            attribute.AttributeType.FullName, attributeFullName, StringComparison.Ordinal)),
    ];

    /// <summary>Refuses an annotated method the weaver cannot instrument.</summary>
    /// <param name="method">The annotated method.</param>
    /// <param name="kind">How the annotation is named in a rejection: "operation" or "setup".</param>
    internal static void RejectUnsupported(MethodDefinition method, string kind)
    {
        if (!method.HasBody)
        {
            throw new WeaveException(
                $"{method.FullName} is annotated as a SpecGate {kind} but has no body.");
        }

        if (HasAttribute(method, AsyncStateMachineAttribute))
        {
            throw new WeaveException(
                $"{method.FullName} is an async {kind}. C# async capture is not supported yet; "
                + $"see issue #71. Remove the annotation or make the {kind} synchronous.");
        }

        if (HasAttribute(method, IteratorStateMachineAttribute))
        {
            throw new WeaveException(
                $"{method.FullName} is an iterator {kind}. Iterator capture is not supported; "
                + "return a materialized collection instead.");
        }

        // Capture resolves the method by its handle and reads what its values
        // mean from reflection. A generic method or a generic declaring type has
        // no single resolvable signature, and discovery cannot publish a type
        // parameter in the registry either, so both are refused here.
        if (method.HasGenericParameters)
        {
            throw new WeaveException(
                $"{method.FullName} is a generic method. SpecGate {kind}s must have concrete "
                + $"parameter and return types; declare the {kind} on a non-generic method.");
        }

        if (method.DeclaringType.HasGenericParameters)
        {
            throw new WeaveException(
                $"{method.FullName} is declared on generic type '{method.DeclaringType.FullName}'. "
                + $"SpecGate {kind}s must be declared on a non-generic type.");
        }
    }

    /// <summary>
    /// The parameters recorded as inputs, in declaration order. Their recorded
    /// names come from reflection at capture time, not from here.
    /// </summary>
    internal static ParameterDefinition[] ReadInputs(MethodDefinition method, string kind)
    {
        foreach (ParameterDefinition parameter in method.Parameters)
        {
            if (parameter.ParameterType.IsByReference || parameter.ParameterType.IsPointer)
            {
                throw new WeaveException(
                    $"{method.FullName} has by-reference parameter '{parameter.Name}'. "
                    + $"SpecGate {kind}s must take parameters by value.");
            }
        }

        return [.. method.Parameters];
    }

    private static bool HasAttribute(MethodDefinition method, string fullName) =>
        method.CustomAttributes.Any(
            attribute => string.Equals(attribute.AttributeType.FullName, fullName, StringComparison.Ordinal));
}
