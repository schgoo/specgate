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
    private const string AsyncIteratorStateMachineAttribute =
        "System.Runtime.CompilerServices.AsyncIteratorStateMachineAttribute";

    /// <summary>
    /// The awaitable a woven async operation returns, which selects the runtime
    /// observer that completes it, or <see cref="AsyncShape.None"/> when the
    /// method is not async.
    /// </summary>
    internal static AsyncShape ShapeOf(MethodDefinition method)
    {
        // Selected by return type, not by the async modifier: a forwarding
        // method returning a Task is still pending when it returns.
        TypeReference type = method.ReturnType;
        return (type.Namespace, type.Name, type.IsGenericInstance) switch
        {
            ("System.Threading.Tasks", "Task", false) => AsyncShape.Task,
            ("System.Threading.Tasks", "Task`1", true) => AsyncShape.TaskOfT,
            ("System.Threading.Tasks", "ValueTask", false) => AsyncShape.ValueTask,
            ("System.Threading.Tasks", "ValueTask`1", true) => AsyncShape.ValueTaskOfT,
            _ => HasAttribute(method, AsyncStateMachineAttribute)
                ? AsyncShape.Unsupported
                : AsyncShape.None,
        };
    }

    private static bool IsSupportedAsync(MethodDefinition method, string kind) =>
        string.Equals(kind, "operation", StringComparison.Ordinal)
        && ShapeOf(method) is not (AsyncShape.None or AsyncShape.Unsupported);

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

        if (HasAttribute(method, AsyncStateMachineAttribute) && !IsSupportedAsync(method, kind))
        {
            throw new WeaveException(
                $"{method.FullName} is an async {kind} returning '{method.ReturnType.FullName}'. "
                + "Async capture supports operations returning Task, Task<T>, ValueTask, or "
                + "ValueTask<T>; async setups, async void, and other awaitable types are not captured.");
        }

        if (HasAttribute(method, AsyncIteratorStateMachineAttribute))
        {
            throw new WeaveException(
                $"{method.FullName} is an async iterator {kind}. Stream capture is not supported; "
                + "return a materialized collection instead.");
        }

        if (HasAttribute(method, IteratorStateMachineAttribute))
        {
            throw new WeaveException(
                $"{method.FullName} is an iterator {kind}. Iterator capture is not supported; "
                + "return a materialized collection instead.");
        }

        if (method.HasGenericParameters && !string.Equals(kind, "operation", StringComparison.Ordinal))
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
