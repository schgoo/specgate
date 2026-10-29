using System;
using System.Collections.Generic;

namespace SpecGate.Runtime;

internal static class InputFolding
{
    internal static IEnumerable<KeyValuePair<string, Value>> Project(
        SemanticInput input, object? value)
    {
        Validate(input.Type, []);
        return Project(input.Name, input.Type, value);
    }

    private static void Validate(SemanticType type, HashSet<Type> visiting)
    {
        if (type.Declared is not Type { IsEnum: false, IsAbstract: false } declared
            || !SemanticType.IsPropertySetup(declared))
        {
            return;
        }
        if (!visiting.Add(declared))
        {
            throw new SpecGateCaptureException(
                CaptureRejection.AmbiguousDeclaration,
                $"specgate: recursive property setup type '{declared.FullName}'.");
        }
        foreach (SemanticMember member in SemanticType.MembersOf(declared))
        {
            Validate(member.Type, visiting);
        }
        visiting.Remove(declared);
    }

    private static IEnumerable<KeyValuePair<string, Value>> Project(
        string name, SemanticType type, object? value)
    {
        if (type.Declared is not Type { IsEnum: false, IsAbstract: false } declared
            || !SemanticType.IsPropertySetup(declared))
        {
            yield return new(name, ValueProjector.Project(type, value));
            yield break;
        }
        if (value is null)
        {
            throw new SpecGateCaptureException(
                CaptureRejection.NullValue,
                $"specgate: property setup parameter '{name}' requires a non-null '{declared.FullName}'.");
        }
        if (!declared.IsInstanceOfType(value))
        {
            throw new SpecGateCaptureException(
                CaptureRejection.TypeMismatch,
                $"specgate: property setup parameter '{name}' requires '{declared.FullName}', "
                + $"but received '{value.GetType().FullName}'.");
        }
        foreach (SemanticMember member in SemanticType.MembersOf(declared))
        {
            foreach (KeyValuePair<string, Value> leaf in Project(
                member.Name, member.Type, member.Read(value)))
            {
                yield return leaf;
            }
        }
    }
}
