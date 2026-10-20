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

    private OperationDeclaration(string operation, string component, ParameterDefinition[] inputs)
    {
        Operation = operation;
        Component = component;
        Inputs = inputs;
    }

    internal string Operation { get; }

    /// <summary>Gets the one component that owns this operation.</summary>
    internal string Component { get; }

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

        if (annotations.Count > 1)
        {
            string components = string.Join(", ", annotations.Select(annotation => DescribeComponent(annotation)));
            throw new WeaveException(
                $"{method.FullName} carries {annotations.Count} SpecOperation annotations ({components}). "
                + "An operation belongs to exactly one component, so a method may declare only one; "
                + "split it into one method per component.");
        }

        CustomAttribute only = annotations[0];
        return new OperationDeclaration(
            ReadOperationName(method, only),
            ReadComponent(method, only),
            AnnotatedMethod.ReadInputs(method, Kind));
    }

    private static string DescribeComponent(CustomAttribute annotation)
    {
        foreach (CustomAttributeNamedArgument property in annotation.Properties)
        {
            if (string.Equals(property.Name, "Spec", StringComparison.Ordinal)
                && property.Argument.Value is string component)
            {
                return $"'{component}'";
            }
        }

        return "no component";
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
