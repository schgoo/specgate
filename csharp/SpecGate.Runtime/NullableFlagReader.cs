using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;

namespace SpecGate.Runtime;

/// <summary>
/// Reads the nullability of one type reference position at a time, in the order
/// the C# compiler recorded them.
/// </summary>
/// <remarks>
/// <para>
/// The compiler flattens a type's nullability into a <c>NullableAttribute</c>
/// byte sequence, visited in pre-order: 0 means oblivious, 1 not nullable, and 2
/// nullable. Only some positions consume a byte, per the compiler's own
/// encoding, and <see cref="SemanticType"/> walks types in exactly that order so
/// the two stay aligned.
/// </para>
/// <para>
/// Two shorthands exist. A sequence whose bytes are all equal is written as a
/// single byte, and an omitted attribute falls back to the nearest enclosing
/// <c>NullableContextAttribute</c>, or to oblivious when there is none. Both
/// collapse to a constant fallback here.
/// </para>
/// <para>
/// <c>NullabilityInfoContext</c> would do this, but it does not expose the
/// declared flag of a generic type argument the way the metadata encoding here
/// does, so the metadata is read directly.
/// </para>
/// </remarks>
internal sealed class NullableFlagReader
{
    private const string NullableAttribute = "System.Runtime.CompilerServices.NullableAttribute";
    private const string NullableContextAttribute =
        "System.Runtime.CompilerServices.NullableContextAttribute";

    private readonly byte[]? _flags;
    private readonly byte _fallback;
    private int _index;

    private NullableFlagReader(byte[]? flags, byte fallback)
    {
        _flags = flags;
        _fallback = fallback;
    }

    /// <summary>Gets a reader that reports every position as oblivious.</summary>
    internal static NullableFlagReader Oblivious { get; } = new(null, 0);

    /// <summary>Reads the next position's nullability.</summary>
    internal byte Next() =>
        _flags is not null && _index < _flags.Length ? _flags[_index++] : _fallback;

    /// <summary>Gets whether the next position is annotated nullable.</summary>
    internal bool NextIsNullable() => Next() == 2;

    /// <summary>Creates a reader for a parameter or a method's return value.</summary>
    internal static NullableFlagReader ForParameter(ParameterInfo parameter, MemberInfo method) =>
        Create(parameter.CustomAttributes, ContextOf(method));

    /// <summary>Creates a reader for a property or field.</summary>
    internal static NullableFlagReader ForMember(MemberInfo member) =>
        Create(member.CustomAttributes, ContextOf(member));

    private static NullableFlagReader Create(IEnumerable<CustomAttributeData> attributes, byte context)
    {
        foreach (CustomAttributeData attribute in attributes)
        {
            if (!string.Equals(attribute.AttributeType.FullName, NullableAttribute, StringComparison.Ordinal)
                || attribute.ConstructorArguments.Count != 1)
            {
                continue;
            }

            object? argument = attribute.ConstructorArguments[0].Value;
            if (argument is byte single)
            {
                return new NullableFlagReader(null, single);
            }

            if (argument is IReadOnlyList<CustomAttributeTypedArgument> sequence)
            {
                return new NullableFlagReader(
                    [.. sequence.Select(element => element.Value is byte flag ? flag : (byte)0)],
                    0);
            }
        }

        return new NullableFlagReader(null, context);
    }

    /// <summary>
    /// Finds the nullable context that applies to <paramref name="member"/>: the
    /// nearest <c>NullableContextAttribute</c> on the member itself, then on each
    /// enclosing type outwards.
    /// </summary>
    private static byte ContextOf(MemberInfo member)
    {
        for (MemberInfo? scope = member; scope is not null; scope = scope.DeclaringType)
        {
            foreach (CustomAttributeData attribute in scope.CustomAttributes)
            {
                if (string.Equals(
                        attribute.AttributeType.FullName, NullableContextAttribute, StringComparison.Ordinal)
                    && attribute.ConstructorArguments.Count == 1
                    && attribute.ConstructorArguments[0].Value is byte flag)
                {
                    return flag;
                }
            }
        }

        return 0;
    }
}
