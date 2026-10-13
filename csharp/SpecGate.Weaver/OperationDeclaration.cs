using System;
using System.Collections.Generic;
using System.Linq;
using Mono.Cecil;

namespace SpecGate.Weaver;

/// <summary>
/// The SpecGate annotations the weaver reads off a compiled method.
/// </summary>
internal sealed class OperationDeclaration
{
    private const string SpecOperationAttribute = "SpecGate.Annotations.SpecOperationAttribute";
    private const string AsyncStateMachineAttribute =
        "System.Runtime.CompilerServices.AsyncStateMachineAttribute";
    private const string IteratorStateMachineAttribute =
        "System.Runtime.CompilerServices.IteratorStateMachineAttribute";

    private OperationDeclaration(string operation, string[] components, ParameterDefinition[] inputs)
    {
        Operation = operation;
        Components = components;
        Inputs = inputs;
    }

    internal string Operation { get; }

    /// <summary>Gets the components declaring this operation, in source order.</summary>
    internal string[] Components { get; }

    /// <summary>
    /// Gets the parameters recorded as inputs, in declaration order. Their
    /// recorded names come from reflection at capture time, not from here.
    /// </summary>
    internal ParameterDefinition[] Inputs { get; }

    /// <summary>
    /// Reads the operation declaration from <paramref name="method"/>, or returns
    /// <see langword="null"/> when the method carries no operation annotation.
    /// </summary>
    /// <exception cref="WeaveException">The method is annotated but cannot be woven.</exception>
    internal static OperationDeclaration? TryRead(MethodDefinition method)
    {
        List<CustomAttribute> annotations =
        [
            .. method.CustomAttributes.Where(attribute => string.Equals(
                attribute.AttributeType.FullName, SpecOperationAttribute, StringComparison.Ordinal)),
        ];
        if (annotations.Count == 0)
        {
            return null;
        }

        RejectUnsupported(method);

        string? operation = null;
        List<string> components = [];
        foreach (CustomAttribute annotation in annotations)
        {
            string name = ReadOperationName(method, annotation);
            if (operation is null)
            {
                operation = name;
            }
            else if (!string.Equals(operation, name, StringComparison.Ordinal))
            {
                throw new WeaveException(
                    $"{method.FullName} declares operations '{operation}' and '{name}'. A method may be "
                    + "declared for several components but must use one operation name.");
            }

            components.Add(ReadComponent(method, annotation));
        }

        string[] distinct = [.. components.Distinct(StringComparer.Ordinal)];
        if (distinct.Length != components.Count)
        {
            throw new WeaveException(
                $"{method.FullName} declares operation '{operation}' more than once for the same component.");
        }

        return new OperationDeclaration(operation!, distinct, ReadInputs(method));
    }

    private static void RejectUnsupported(MethodDefinition method)
    {
        if (!method.HasBody)
        {
            throw new WeaveException(
                $"{method.FullName} is annotated as a SpecGate operation but has no body.");
        }

        if (HasAttribute(method, AsyncStateMachineAttribute))
        {
            throw new WeaveException(
                $"{method.FullName} is an async operation. C# async capture is not supported yet; "
                + "see issue #71. Remove the annotation or make the operation synchronous.");
        }

        if (HasAttribute(method, IteratorStateMachineAttribute))
        {
            throw new WeaveException(
                $"{method.FullName} is an iterator operation. Iterator capture is not supported; "
                + "return a materialized collection instead.");
        }

        // Capture resolves the method by its handle and reads what its values
        // mean from reflection. A generic method or a generic declaring type has
        // no single resolvable signature, and discovery cannot publish a type
        // parameter in the registry either, so both are refused here.
        if (method.HasGenericParameters)
        {
            throw new WeaveException(
                $"{method.FullName} is a generic method. SpecGate operations must have concrete "
                + "parameter and return types; declare the operation on a non-generic method.");
        }

        if (method.DeclaringType.HasGenericParameters)
        {
            throw new WeaveException(
                $"{method.FullName} is declared on generic type '{method.DeclaringType.FullName}'. "
                + "SpecGate operations must be declared on a non-generic type.");
        }
    }

    private static bool HasAttribute(MethodDefinition method, string fullName)
    {
        return method.CustomAttributes.Any(
            attribute => string.Equals(attribute.AttributeType.FullName, fullName, StringComparison.Ordinal));
    }

    private static string ReadOperationName(MethodDefinition method, CustomAttribute annotation)
    {
        if (annotation.ConstructorArguments.Count != 1
            || annotation.ConstructorArguments[0].Value is not string name
            || name.Length == 0)
        {
            throw new WeaveException($"{method.FullName} has a SpecOperation annotation without a name.");
        }

        return name;
    }

    private static string ReadComponent(MethodDefinition method, CustomAttribute annotation)
    {
        foreach (CustomAttributeNamedArgument property in annotation.Properties)
        {
            if (string.Equals(property.Name, "Spec", StringComparison.Ordinal)
                && property.Argument.Value is string component
                && component.Length > 0)
            {
                return component;
            }
        }

        throw new WeaveException(
            $"{method.FullName} has a SpecOperation annotation without a Spec component. "
            + "Capture selects operations by component, so the component is required.");
    }

    private static ParameterDefinition[] ReadInputs(MethodDefinition method)
    {
        foreach (ParameterDefinition parameter in method.Parameters)
        {
            if (parameter.ParameterType.IsByReference || parameter.ParameterType.IsPointer)
            {
                throw new WeaveException(
                    $"{method.FullName} has by-reference parameter '{parameter.Name}'. "
                    + "SpecGate operations must take parameters by value.");
            }
        }

        return [.. method.Parameters];
    }
}
