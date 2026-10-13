using System;
using System.Collections.Generic;
using System.Linq;
using Mono.Cecil;

namespace SpecGate.Weaver;

/// <summary>
/// The SpecGate operation annotations the weaver reads off a compiled method.
/// </summary>
internal sealed class OperationDeclaration
{
    private const string Kind = "operation";

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
            AnnotatedMethod.Annotations(method, AnnotatedMethod.SpecOperationAttribute);
        if (annotations.Count == 0)
        {
            return null;
        }

        AnnotatedMethod.RejectUnsupported(method, Kind);

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

        return new OperationDeclaration(operation!, distinct, AnnotatedMethod.ReadInputs(method, Kind));
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
}
