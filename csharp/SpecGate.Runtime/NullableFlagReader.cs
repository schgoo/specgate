using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using SpecGate.Reflection;

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

    internal NullableFlagReader Fork() => new(_flags, _fallback) { _index = _index };

    internal void Skip(Type type)
    {
        if (Nullable.GetUnderlyingType(type) is Type inner)
        {
            Skip(inner);
            return;
        }
        if (!type.IsValueType || type.IsGenericType)
        {
            Next();
        }
        if (type.IsArray)
        {
            Skip(type.GetElementType()!);
        }
        else
        {
            foreach (Type argument in type.GetGenericArguments())
            {
                Skip(argument);
            }
        }
    }

    /// <summary>Creates a reader for a parameter or a method's return value.</summary>
    internal static NullableFlagReader ForParameter(ParameterInfo parameter, MemberInfo method) =>
        Create(parameter.CustomAttributes, ContextOf(method));

    /// <summary>Creates a reader for a property or field.</summary>
    internal static NullableFlagReader ForMember(MemberInfo member)
    {
        MemberInfo declaration = RecordMembers.DeclarationOf(member);
        NullableFlagReader source = Create(declaration.CustomAttributes, ContextOf(declaration));
        if (declaration == member)
        {
            return source;
        }

        List<byte> flags = [];
        Substitute(MemberType(declaration), MemberType(member));
        return new NullableFlagReader([.. flags], 0);

        // Metadata positions belong to the open signature: T consumes one
        // position even when its closed substitution (such as int) consumes none.
        void Substitute(Type open, Type closed)
        {
            if (open.IsGenericParameter)
            {
                byte flag = HasPosition(open) ? source.Next() : (byte)0;
                Append(closed, flag);
                return;
            }
            if (Nullable.GetUnderlyingType(open) is Type inner)
            {
                Substitute(inner, Nullable.GetUnderlyingType(closed)!);
                return;
            }
            if (HasPosition(open))
            {
                flags.Add(source.Next());
            }
            if (open.IsArray)
            {
                Substitute(open.GetElementType()!, closed.GetElementType()!);
            }
            else
            {
                Type[] arguments = open.GetGenericArguments();
                Type[] substitutions = closed.GetGenericArguments();
                for (int index = 0; index < arguments.Length; index++)
                {
                    Substitute(arguments[index], substitutions[index]);
                }
            }
        }

        void Append(Type closed, byte flag)
        {
            if (Nullable.GetUnderlyingType(closed) is Type inner)
            {
                Append(inner, flag);
                return;
            }
            if (HasPosition(closed))
            {
                flags.Add(flag);
            }
            if (closed.IsArray)
            {
                Append(closed.GetElementType()!, 0);
            }
            else
            {
                // A member's T annotation describes T itself, not the nullable
                // arguments of a type supplied by an inheritance use site.
                foreach (Type argument in closed.GetGenericArguments())
                {
                    Append(argument, 0);
                }
            }
        }
    }

    private static bool HasPosition(Type type) => !type.IsValueType || type.IsGenericType;

    private static Type MemberType(MemberInfo member) => member switch
    {
        PropertyInfo property => property.PropertyType,
        FieldInfo field => field.FieldType,
        _ => throw new ArgumentException("Expected a property or field.", nameof(member)),
    };

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
