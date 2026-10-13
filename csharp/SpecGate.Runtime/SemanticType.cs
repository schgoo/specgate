using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using SpecGate.Annotations;

namespace SpecGate.Runtime;

/// <summary>The CTSC type families a C# declaration can map onto.</summary>
internal enum SemanticKind
{
    Bool,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    String,

    /// <summary>A declared <see cref="object"/>, whose family is decided per value.</summary>
    Dynamic,
    List,
    Set,
    Map,
    Option,
    Result,

    /// <summary>A type annotated with <see cref="SpecEventAttribute"/>.</summary>
    Named,
}

/// <summary>
/// The CTSC meaning of one C# declaration: a parameter, a return value, or an
/// annotated member.
/// </summary>
/// <remarks>
/// This is the single place C# declarations are interpreted. The same mapping
/// has to hold for the whole tree, because an annotated type's members are only
/// reachable through the runtime type of the value, so the weaver cannot
/// precompute them. Keeping one mapper here instead of splitting it between the
/// weaver and the runtime is what keeps capture consistent with the registry
/// that <c>specgate discover</c> publishes.
/// </remarks>
internal sealed class SemanticType
{
    private static readonly ConcurrentDictionary<Type, SemanticMember[]> MemberCache = new();

    private SemanticType(SemanticKind kind, SemanticType? first, SemanticType? second, Type? declared)
    {
        Kind = kind;
        First = first;
        Second = second;
        Declared = declared;
    }

    internal SemanticKind Kind { get; }

    /// <summary>The element type, the optional's value, the map's key, or the result's success type.</summary>
    internal SemanticType? First { get; }

    /// <summary>The map's value type or the result's error type.</summary>
    internal SemanticType? Second { get; }

    /// <summary>The annotated CLR type, for <see cref="SemanticKind.Named"/>.</summary>
    internal Type? Declared { get; }

    private static SemanticType Scalar(SemanticKind kind) => new(kind, null, null, null);

    private static SemanticType Of(SemanticKind kind, SemanticType first, SemanticType? second = null) =>
        new(kind, first, second, null);

    internal static SemanticType Named(Type declared) => new(SemanticKind.Named, null, null, declared);

    /// <summary>Maps a parameter or return value declaration.</summary>
    internal static SemanticType ForParameter(ParameterInfo parameter, MemberInfo method) =>
        Map(parameter.ParameterType, NullableFlagReader.ForParameter(parameter, method));

    /// <summary>
    /// Maps one declared type, consuming the nullability positions it occupies.
    /// </summary>
    /// <remarks>
    /// The traversal order must match the compiler's: a reference or generic
    /// value type contributes its own nullability byte before its type arguments,
    /// a non-generic value type contributes none, and <c>T?</c> contributes only
    /// whatever <c>T</c> contributes.
    /// </remarks>
    internal static SemanticType Map(Type type, NullableFlagReader flags)
    {
        Type? underlying = Nullable.GetUnderlyingType(type);
        if (underlying is not null)
        {
            return Of(SemanticKind.Option, Map(underlying, flags));
        }

        if (type.IsArray)
        {
            bool nullable = flags.NextIsNullable();
            SemanticType list = Of(SemanticKind.List, Map(type.GetElementType()!, flags));
            return nullable ? Of(SemanticKind.Option, list) : list;
        }

        if (!type.IsValueType)
        {
            bool nullable = flags.NextIsNullable();
            SemanticType core = MapCore(type, flags);
            return nullable ? Of(SemanticKind.Option, core) : core;
        }

        if (type.IsGenericType)
        {
            flags.Next();
        }

        return MapCore(type, flags);
    }

    private static SemanticType MapCore(Type type, NullableFlagReader flags)
    {
        if (type == typeof(bool))
        {
            return Scalar(SemanticKind.Bool);
        }

        if (type == typeof(string))
        {
            return Scalar(SemanticKind.String);
        }

        if (type == typeof(object))
        {
            return Scalar(SemanticKind.Dynamic);
        }

        if (type == typeof(sbyte))
        {
            return Scalar(SemanticKind.I8);
        }

        if (type == typeof(short))
        {
            return Scalar(SemanticKind.I16);
        }

        if (type == typeof(int))
        {
            return Scalar(SemanticKind.I32);
        }

        if (type == typeof(long))
        {
            return Scalar(SemanticKind.I64);
        }

        if (type == typeof(float))
        {
            return Scalar(SemanticKind.F32);
        }

        if (type == typeof(double))
        {
            return Scalar(SemanticKind.F64);
        }

        if (type.IsGenericType)
        {
            Type definition = type.GetGenericTypeDefinition();
            Type[] arguments = type.GetGenericArguments();

            if (definition == typeof(Option<>))
            {
                return Of(SemanticKind.Option, Map(arguments[0], flags));
            }

            if (definition == typeof(Result<,>))
            {
                return Of(SemanticKind.Result, Map(arguments[0], flags), Map(arguments[1], flags));
            }

            if (definition == typeof(List<>))
            {
                return Of(SemanticKind.List, Map(arguments[0], flags));
            }

            if (definition == typeof(Dictionary<,>) || definition == typeof(SortedDictionary<,>))
            {
                return Of(SemanticKind.Map, Map(arguments[0], flags), Map(arguments[1], flags));
            }

            if (definition == typeof(HashSet<>) || definition == typeof(SortedSet<>))
            {
                return Of(SemanticKind.Set, Map(arguments[0], flags));
            }
        }

        if (HasSpecEvent(type))
        {
            return Named(type);
        }

        throw new SpecGateCaptureException(
            CaptureRejection.UnsupportedType,
            $"specgate: '{type.FullName}' has no CTSC representation. Use a supported scalar, "
            + "List, Dictionary, SortedDictionary, HashSet, SortedSet, Option, Result, or a type "
            + "annotated with [SpecEvent].");
    }

    /// <summary>Gets the semantic name a <see cref="SpecEventAttribute"/> declares.</summary>
    internal static string EventName(MemberInfo member)
    {
        SpecEventAttribute? annotation = member.GetCustomAttribute<SpecEventAttribute>(inherit: false);
        string? declared = annotation?.Name;
        return string.IsNullOrEmpty(declared) ? member.Name : declared!;
    }

    internal static bool HasSpecEvent(Type type) =>
        type.GetCustomAttribute<SpecEventAttribute>(inherit: false) is not null;

    /// <summary>
    /// Gets the annotated members of <paramref name="type"/>, matching the
    /// selection <c>specgate discover</c> publishes: public instance members
    /// declared on the type itself, properties before fields, each carrying
    /// <see cref="SpecEventAttribute"/>.
    /// </summary>
    internal static SemanticMember[] MembersOf(Type type) => MemberCache.GetOrAdd(type, Describe);

    private static SemanticMember[] Describe(Type type)
    {
        const BindingFlags Selection =
            BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly;

        List<SemanticMember> members = [];
        foreach (PropertyInfo property in type.GetProperties(Selection))
        {
            if (HasSpecEvent(property) && property.CanRead)
            {
                members.Add(new SemanticMember(
                    EventName(property),
                    Map(property.PropertyType, NullableFlagReader.ForMember(property)),
                    instance => property.GetValue(instance)));
            }
        }

        foreach (FieldInfo field in type.GetFields(Selection))
        {
            if (HasSpecEvent(field))
            {
                members.Add(new SemanticMember(
                    EventName(field),
                    Map(field.FieldType, NullableFlagReader.ForMember(field)),
                    field.GetValue));
            }
        }

        string[] duplicates =
        [
            .. members.GroupBy(member => member.Name, StringComparer.Ordinal)
                .Where(group => group.Count() > 1)
                .Select(group => group.Key),
        ];
        if (duplicates.Length > 0)
        {
            throw new SpecGateCaptureException(
                CaptureRejection.AmbiguousDeclaration,
                $"specgate: '{type.FullName}' declares the member name(s) "
                + $"'{string.Join("', '", duplicates)}' more than once.");
        }

        return [.. members];
    }

    private static bool HasSpecEvent(MemberInfo member) =>
        member.GetCustomAttribute<SpecEventAttribute>(inherit: false) is not null;
}

/// <summary>One annotated member of a recorded type.</summary>
internal sealed class SemanticMember(string name, SemanticType type, Func<object, object?> read)
{
    internal string Name { get; } = name;

    internal SemanticType Type { get; } = type;

    internal Func<object, object?> Read { get; } = read;
}
