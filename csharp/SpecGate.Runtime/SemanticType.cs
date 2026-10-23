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
    U32,
    U64,
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
internal sealed class SemanticType : IEquatable<SemanticType>
{
    private static readonly ConcurrentDictionary<Type, SemanticMember[]> MemberCache = new();

    private static readonly ConcurrentDictionary<Type, Dictionary<string, string>> VariantCache = new();

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

        if (type == typeof(uint))
        {
            return Scalar(SemanticKind.U32);
        }

        if (type == typeof(ulong))
        {
            return Scalar(SemanticKind.U64);
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

    /// <summary>
    /// Whether two declarations mean the same CTSC type.
    /// </summary>
    /// <remarks>
    /// Setup folding matches a producer's return type against an operation's
    /// parameters. Rust compares the whitespace-stripped declared type strings
    /// (<c>rust/crates/specgate-runtime/src/capture/setup.rs:393-407</c>); the C#
    /// equivalent is the mapped type rather than the CLR type, because two CLR
    /// types can carry the same <see cref="SpecEventAttribute"/> name and
    /// discovery folds on the registry-visible name.
    /// </remarks>
    public bool Equals(SemanticType? other)
    {
        if (other is null)
        {
            return false;
        }

        if (ReferenceEquals(this, other))
        {
            return true;
        }

        if (Kind != other.Kind)
        {
            return false;
        }

        if (Kind == SemanticKind.Named)
        {
            return string.Equals(EventName(Declared!), EventName(other.Declared!), StringComparison.Ordinal);
        }

        return Same(First, other.First) && Same(Second, other.Second);
    }

    public override bool Equals(object? obj) => Equals(obj as SemanticType);

    public override int GetHashCode()
    {
        unchecked
        {
            int hash = (17 * 31) + (int)Kind;
            if (Kind == SemanticKind.Named)
            {
                return (hash * 31) + EventName(Declared!).GetHashCode();
            }

            hash = (hash * 31) + (First?.GetHashCode() ?? 0);
            return (hash * 31) + (Second?.GetHashCode() ?? 0);
        }
    }

    private static bool Same(SemanticType? left, SemanticType? right) =>
        left is null ? right is null : left.Equals(right);

    /// <summary>Gets the semantic name a <see cref="SpecEventAttribute"/> declares.</summary>
    /// <remarks>
    /// Only a missing name falls back to the CLR name, matching discovery's
    /// <c>SpecEventName(member) ?? member.Name</c>. An explicitly declared empty
    /// name is preserved, so record keys, type names, and variant names stay
    /// identical between the registry and captured values.
    /// </remarks>
    internal static string EventName(MemberInfo member)
    {
        SpecEventAttribute? annotation = member.GetCustomAttribute<SpecEventAttribute>(inherit: false);
        return annotation?.Name ?? member.Name;
    }

    internal static bool HasSpecEvent(Type type) =>
        type.GetCustomAttribute<SpecEventAttribute>(inherit: false) is not null;

    internal static bool IsPropertySetup(Type type) =>
        type.GetCustomAttribute<SpecEventAttribute>(inherit: false)?.Setup == true;

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
        bool all = IsPropertySetup(type)
            && !type.GetProperties(Selection).Cast<MemberInfo>().Concat(type.GetFields(Selection)).Any(HasSpecEvent);
        foreach (PropertyInfo property in type.GetProperties(Selection))
        {
            if ((all || HasSpecEvent(property)) && (property.CanRead || IsPropertySetup(type)))
            {
                if (IsPropertySetup(type)
                    && (property.GetMethod is not { IsPublic: true } || property.GetIndexParameters().Length != 0))
                {
                    throw new SpecGateCaptureException(CaptureRejection.UnsupportedType,
                        $"specgate: property setup member '{type.FullName}.{property.Name}' must have a public non-indexed getter.");
                }
                members.Add(new SemanticMember(
                    EventName(property),
                    Map(property.PropertyType, NullableFlagReader.ForMember(property)),
                    instance => property.GetValue(instance)));
            }
        }

        foreach (FieldInfo field in type.GetFields(Selection))
        {
            if (all || HasSpecEvent(field))
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

    /// <summary>
    /// Gets the variant name recorded for one enum value, or <see langword="null"/>
    /// when the value is not exactly one declared member.
    /// </summary>
    /// <remarks>
    /// The name comes from <see cref="Enum.GetName(Type, object)"/>, which is
    /// deterministic but cannot distinguish an alias from the member it aliases:
    /// <c>Default = Low</c> and <c>Low</c> share one underlying value, and a
    /// CTSC variant has no slot for a discriminant to tell them apart. Both
    /// therefore record the one name the lookup returns. Variant order is not
    /// decided here; the discovery pass publishes the declared order.
    /// </remarks>
    internal static string? VariantOf(Type type, object value)
    {
        string? member = Enum.GetName(type, value);
        return member is not null
            && VariantCache.GetOrAdd(type, DescribeEnum).TryGetValue(member, out string? name)
            ? name
            : null;
    }

    /// <summary>
    /// Maps each declared member of an enum onto its semantic variant name.
    /// </summary>
    /// <remarks>
    /// The members of an enum are its public static declared fields, read
    /// through reflection so that a <see cref="SpecEventAttribute"/> name
    /// override is honored. The compiler-generated backing field is
    /// special-named and is not one of them.
    /// <para>
    /// A CTSC variant is identified by its name alone, so two members that
    /// resolve to one variant name are rejected as an ambiguous declaration,
    /// mirroring the rule applied to the members of a record.
    /// </para>
    /// </remarks>
    private static Dictionary<string, string> DescribeEnum(Type type)
    {
        const BindingFlags Selection =
            BindingFlags.Public | BindingFlags.Static | BindingFlags.DeclaredOnly;

        Dictionary<string, string> byMemberName = new(StringComparer.Ordinal);
        HashSet<string> declared = new(StringComparer.Ordinal);
        foreach (FieldInfo field in type.GetFields(Selection))
        {
            if (field.IsSpecialName)
            {
                continue;
            }

            string name = EventName(field);
            if (!declared.Add(name))
            {
                throw new SpecGateCaptureException(
                    CaptureRejection.AmbiguousDeclaration,
                    $"specgate: '{type.FullName}' declares the variant name '{name}' more than "
                    + "once. A CTSC variant is identified by its name alone.");
            }

            byMemberName[field.Name] = name;
        }

        return byMemberName;
    }
}

/// <summary>One annotated member of a recorded type.</summary>
internal sealed class SemanticMember(string name, SemanticType type, Func<object, object?> read)
{
    internal string Name { get; } = name;

    internal SemanticType Type { get; } = type;

    internal Func<object, object?> Read { get; } = read;
}
