using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Reflection;
using SpecGate.Annotations;
using SpecGate.Collections;

namespace SpecGate.Runtime;

/// <summary>
/// Converts a live CLR value into the CTSC <see cref="Value"/> that capture
/// records.
/// </summary>
/// <remarks>
/// <para>
/// Projection happens at the operation boundary, while the call is on the stack.
/// Holding the original object and converting later would record whatever the
/// object was mutated into afterwards, not what the operation actually saw.
/// </para>
/// <para>
/// The shapes here mirror the Rust runtime exactly, because a C# reference
/// capture and a Rust one have to be comparable: a record becomes a map of its
/// members, a tagged union becomes a single-entry map from variant name to its
/// payload, a payload-free variant's payload is the empty map, an optional
/// becomes <c>Some</c> or <c>None</c>, and a result becomes <c>Ok</c> or
/// <c>Err</c>.
/// </para>
/// </remarks>
internal static class ValueProjector
{
    private const string SomeKey = "Some";
    private const string NoneKey = "None";
    private const string OkKey = "Ok";
    private const string ErrKey = "Err";

    internal static Value Project(SemanticType type, object? value)
    {
        if (type.Kind == SemanticKind.Option)
        {
            return ProjectOption(type.First!, value);
        }

        if (value is null)
        {
            throw Rejected(
                CaptureRejection.NullValue,
                $"a null was observed where {Describe(type)} is required. Declare the "
                + "parameter, member, or return value nullable to record absence.");
        }

        return type.Kind switch
        {
            SemanticKind.Bool => Value.Bool(Require<bool>(type, value)),
            SemanticKind.I8 => Value.Integer(Require<sbyte>(type, value)),
            SemanticKind.I16 => Value.Integer(Require<short>(type, value)),
            SemanticKind.I32 => Value.Integer(Require<int>(type, value)),
            SemanticKind.I64 => Value.Integer(Require<long>(type, value)),
            SemanticKind.U32 => Value.Integer(Require<uint>(type, value)),
            SemanticKind.U64 => Value.Unsigned(Require<ulong>(type, value)),
            SemanticKind.F32 => Value.Float(Require<float>(type, value)),
            SemanticKind.F64 => Value.Float(Require<double>(type, value)),
            SemanticKind.String => ProjectString(Require<string>(type, value)),
            SemanticKind.Dynamic => ProjectDynamic(value),
            SemanticKind.List or SemanticKind.Bag => Value.List(ProjectElements(type, type.First!, value)),
            SemanticKind.Set or SemanticKind.OrderedSet => ProjectUnique(type, value),
            SemanticKind.Map or SemanticKind.OrderedMap => ProjectMap(type, value),
            SemanticKind.Tuple => ProjectTuple(type, value),
            SemanticKind.Result => ProjectResult(type, value),
            SemanticKind.Named => ProjectNamed(type.Declared!, value),
            _ => throw Rejected(
                CaptureRejection.UnsupportedType, $"{Describe(type)} cannot be recorded."),
        };
    }

    /// <summary>
    /// Projects an operation's outer return value onto the channel Rust records
    /// it on.
    /// </summary>
    /// <remarks>
    /// Discovery unwraps the outermost <c>Option</c> or <c>Result</c> of a
    /// return type (<c>discovery/normalize/schema.rs</c>), and Rust's annotation
    /// macro records the same split: a present option and an <c>Ok</c> are the
    /// <c>result</c> channel carrying the inner value, an absent option is
    /// <c>empty</c>, and an <c>Err</c> is the <c>error</c> channel. Only the
    /// outermost wrapper is unwrapped; inputs and nested values keep the tagged
    /// projection.
    /// </remarks>
    internal static ReturnOutcome ProjectReturn(SemanticType type, object? value)
    {
        if (type.Kind == SemanticKind.Option)
        {
            return TryUnwrapOption(value, out object? held)
                ? new ReturnOutcome(ReturnChannel.Result, Project(type.First!, held))
                : new ReturnOutcome(ReturnChannel.Empty, null);
        }

        if (type.Kind == SemanticKind.Result && value is not null)
        {
            Type actual = value.GetType();
            if (!actual.IsGenericType || actual.GetGenericTypeDefinition() != typeof(Result<,>))
            {
                throw Mismatch(type, actual);
            }

            if ((bool)actual.GetProperty(nameof(Result<int, int>.IsOk))!.GetValue(value)!)
            {
                object? ok = actual.GetProperty(nameof(Result<int, int>.OkValue))!.GetValue(value);
                return new ReturnOutcome(ReturnChannel.Result, Project(type.First!, ok));
            }

            object? error = actual.GetProperty(nameof(Result<int, int>.ErrValue))!.GetValue(value);
            return new ReturnOutcome(ReturnChannel.Error, Project(type.Second!, error));
        }

        return new ReturnOutcome(ReturnChannel.Result, Project(type, value));
    }

    private static Value ProjectOption(SemanticType inner, object? value) =>
        TryUnwrapOption(value, out object? held)
            ? Value.Tagged(SomeKey, Project(inner, held))
            : Value.Tagged(NoneKey, Value.Unit);

    /// <summary>Reads whether an optional value is present, and the value it holds.</summary>
    /// <remarks>
    /// Option&lt;T&gt; and Result&lt;T, E&gt; are structs, so a declared
    /// Option&lt;T&gt; always arrives boxed and non-null; absence lives in
    /// HasValue, not in the box. A nullable's absence is a null.
    /// </remarks>
    private static bool TryUnwrapOption(object? value, out object? held)
    {
        held = null;
        if (value is null)
        {
            return false;
        }

        Type actual = value.GetType();
        if (actual.IsGenericType && actual.GetGenericTypeDefinition() == typeof(Option<>))
        {
            PropertyInfo hasValue = actual.GetProperty(nameof(Option<int>.HasValue))!;
            if (!(bool)hasValue.GetValue(value)!)
            {
                return false;
            }

            held = actual.GetProperty(nameof(Option<int>.Value))!.GetValue(value);
            return true;
        }

        held = value;
        return true;
    }

    private static Value ProjectResult(SemanticType type, object value)
    {
        Type actual = value.GetType();
        if (!actual.IsGenericType || actual.GetGenericTypeDefinition() != typeof(Result<,>))
        {
            throw Mismatch(type, actual);
        }

        if ((bool)actual.GetProperty(nameof(Result<int, int>.IsOk))!.GetValue(value)!)
        {
            object? ok = actual.GetProperty(nameof(Result<int, int>.OkValue))!.GetValue(value);
            return Value.Tagged(OkKey, Project(type.First!, ok));
        }

        object? error = actual.GetProperty(nameof(Result<int, int>.ErrValue))!.GetValue(value);
        return Value.Tagged(ErrKey, Project(type.Second!, error));
    }

    private static Value ProjectNamed(Type declared, object value)
    {
        Type actual = value.GetType();
        if (!declared.IsAssignableFrom(actual))
        {
            throw Rejected(
                CaptureRejection.TypeMismatch,
                $"a '{actual.FullName}' was observed where '{declared.FullName}' is declared.");
        }

        // A plain CLR enum is a tagged union of payload-free variants: the
        // member name over the empty map, matching how the Rust runtime records
        // a unit variant.
        if (declared.IsEnum)
        {
            string? variant = SemanticType.VariantOf(declared, value);
            if (variant is null)
            {
                string discriminant = ((Enum)value).ToString("D");
                throw Rejected(
                    CaptureRejection.UnrepresentableValue,
                    $"the value {discriminant} of '{declared.FullName}' is not one of its declared "
                    + "members. CTSC records an enum as exactly one declared variant.");
            }

            return Value.Tagged(variant, Value.Unit);
        }

        // An abstract annotated base class is a tagged union: each concrete
        // subclass is a variant, recorded as its own name over its own members.
        if (declared.IsAbstract)
        {
            return Value.Tagged(SemanticType.EventName(actual), Record(actual, value));
        }

        return Record(declared, value);
    }

    private static Value Record(Type type, object instance)
    {
        SemanticMember[] members = SemanticType.MembersOf(type);
        KeyValuePair<string, Value>[] entries = new KeyValuePair<string, Value>[members.Length];
        for (int index = 0; index < members.Length; index++)
        {
            SemanticMember member = members[index];
            entries[index] = new KeyValuePair<string, Value>(
                member.Name, Project(member.Type, member.Read(instance)));
        }

        return Value.Map(entries);
    }

    private static List<Value> ProjectElements(SemanticType type, SemanticType element, object value)
    {
        if (type.EnumerationProperty is string property)
        {
            value = value.GetType().GetProperty(property)!.GetValue(value)!;
        }
        if (value is Array array)
        {
            List<Value> elements = [];
            foreach (object? item in array)
            {
                elements.Add(Project(element, item));
            }
            return elements;
        }
        if (value is not IEnumerable || value is string)
        {
            throw Mismatch(type, value.GetType());
        }

        Type elementType = (type.ClrArguments ?? CollectionShape.Resolve(value.GetType())!.Arguments)[0];
        MethodInfo method = typeof(ValueProjector).GetMethod(nameof(ProjectSequence),
            BindingFlags.Static | BindingFlags.NonPublic)!.MakeGenericMethod(elementType);
        return method.CreateDelegate<Func<SemanticType, SemanticType, object, List<Value>>>()(type, element, value);
    }

    private static List<Value> ProjectSequence<T>(SemanticType type, SemanticType element, object value)
    {
        if (value is not IEnumerable<T> sequence)
        {
            throw Mismatch(type, value.GetType());
        }
        List<Value> projected = [];
        foreach (T item in sequence)
        {
            projected.Add(Project(element, item));
        }

        return projected;
    }

    private static Value ProjectUnique(SemanticType type, object value)
    {
        List<Value> elements = ProjectElements(type, type.First!, value);
        if (elements.Select(element => Canonicalize(type.First!, element)).Distinct().Count() != elements.Count)
        {
            throw Rejected(
                CaptureRejection.UnrepresentableValue,
                "distinct native elements project to duplicate elements in a unique CTSC collection.");
        }
        return type.Kind == SemanticKind.OrderedSet ? Value.List(elements) : Value.Set(elements);
    }

    private static Value ProjectTuple(SemanticType type, object value)
    {
        Type actual = value.GetType();
        return Value.List([
            Project(type.First!, actual.GetField("Item1")!.GetValue(value)),
            Project(type.Second!, actual.GetField("Item2")!.GetValue(value)),
        ]);
    }

    private static Value ProjectMap(SemanticType type, object value)
    {
        Type[] arguments = type.ClrArguments ?? CollectionShape.Resolve(value.GetType())!.Arguments;
        MethodInfo method = typeof(ValueProjector).GetMethod(nameof(ProjectEntries),
            BindingFlags.Static | BindingFlags.NonPublic)!.MakeGenericMethod(arguments);
        return method.CreateDelegate<Func<SemanticType, object, Value>>()(type, value);
    }

    private static Value ProjectEntries<TKey, TValue>(SemanticType type, object value)
    {
        if (value is not IEnumerable<KeyValuePair<TKey, TValue>> dictionary)
        {
            throw Mismatch(type, value.GetType());
        }

        List<KeyValuePair<string, Value>> entries = [];
        List<Value> orderedEntries = [];
        HashSet<Value> keys = [];
        bool compact = type.Kind == SemanticKind.Map && type.First!.Kind == SemanticKind.String;
        foreach (KeyValuePair<TKey, TValue> entry in dictionary)
        {
            Value key = Project(type.First!, entry.Key);
            Value projected = Project(type.Second!, entry.Value);
            if (!keys.Add(Canonicalize(type.First!, key)))
            {
                throw Rejected(
                    CaptureRejection.UnrepresentableValue,
                    "distinct native keys project to duplicate CTSC map keys.");
            }
            if (compact)
            {
                entries.Add(new KeyValuePair<string, Value>(key.AsString(), projected));
            }
            else
            {
                orderedEntries.Add(Value.Map([
                    new KeyValuePair<string, Value>("key", key),
                    new KeyValuePair<string, Value>("value", projected),
                ]));
            }
        }

        return compact ? Value.Map(entries) : Value.List(orderedEntries);
    }

    private static Value Canonicalize(SemanticType type, Value value)
    {
        switch (type.Kind)
        {
            case SemanticKind.List:
            case SemanticKind.Bag:
            case SemanticKind.Set:
            case SemanticKind.OrderedSet:
                List<Value> elements = [.. (value.Kind == ValueKind.Set ? value.AsSet() : value.AsList())
                    .Select(item => Canonicalize(type.First!, item))];
                if (type.Kind is SemanticKind.Bag or SemanticKind.Set)
                {
                    elements.Sort(Value.Compare);
                }
                return Value.List(elements);
            case SemanticKind.Tuple:
                return Value.List([
                    Canonicalize(type.First!, value.AsList()[0]),
                    Canonicalize(type.Second!, value.AsList()[1]),
                ]);
            case SemanticKind.Option:
            case SemanticKind.Result:
                KeyValuePair<string, Value> variant = value.AsMap().Single();
                if (type.Kind == SemanticKind.Option && variant.Key == NoneKey)
                {
                    return value;
                }
                return Value.Tagged(variant.Key,
                    Canonicalize(variant.Key == ErrKey ? type.Second! : type.First!, variant.Value));
            case SemanticKind.Map:
            case SemanticKind.OrderedMap:
                if (value.Kind == ValueKind.Map)
                {
                    return Value.Map(value.AsMap().Select(entry =>
                        new KeyValuePair<string, Value>(entry.Key, Canonicalize(type.Second!, entry.Value))));
                }
                List<Value> entries = [.. value.AsList().Select(entry => Value.List([
                    Canonicalize(type.First!, entry.AsMap()["key"]),
                    Canonicalize(type.Second!, entry.AsMap()["value"]),
                ]))];
                if (type.Kind == SemanticKind.Map)
                {
                    entries.Sort(Value.Compare);
                }
                return Value.List(entries);
            case SemanticKind.Named:
                Type declared = type.Declared!;
                if (declared.IsEnum)
                {
                    return value;
                }
                if (declared.IsAbstract)
                {
                    KeyValuePair<string, Value> tagged = value.AsMap().Single();
                    Type variantType = declared.Assembly.GetTypes().Single(candidate =>
                        !candidate.IsAbstract && declared.IsAssignableFrom(candidate)
                        && SemanticType.HasSpecEvent(candidate) && SemanticType.EventName(candidate) == tagged.Key);
                    return Value.Tagged(tagged.Key, Canonicalize(SemanticType.Named(variantType), tagged.Value));
                }
                IReadOnlyDictionary<string, Value> fields = value.AsMap();
                return Value.Map(SemanticType.MembersOf(declared).Select(member =>
                    new KeyValuePair<string, Value>(member.Name, Canonicalize(member.Type, fields[member.Name]))));
            case SemanticKind.F32:
            case SemanticKind.F64:
                return double.IsNaN(value.AsFloat()) ? Value.Float(double.NaN) : value;
            default:
                return value;
        }
    }

    /// <summary>
    /// Projects a value whose declared type is <see cref="object"/>.
    /// </summary>
    /// <remarks>
    /// Only families whose CTSC shape is unambiguous from the runtime type are
    /// accepted. A collection is refused because nothing distinguishes a list
    /// from a set once the declared type is erased, and guessing would make two
    /// captures of the same data disagree.
    /// </remarks>
    private static Value ProjectDynamic(object value) => value switch
    {
        bool flag => Value.Bool(flag),
        sbyte number => Value.Integer(number),
        short number => Value.Integer(number),
        int number => Value.Integer(number),
        long number => Value.Integer(number),
        uint number => Value.Integer(number),
        ulong number => Value.Unsigned(number),
        float number => Value.Float(number),
        double number => Value.Float(number),
        string text => ProjectString(text),
        _ when SemanticType.HasSpecEvent(ResolveDynamicType(value)) =>
            ProjectNamed(ResolveDynamicType(value), value),
        _ => throw Rejected(
            CaptureRejection.UnsupportedType,
            $"a '{value.GetType().FullName}' was observed for a declared 'object'. Declare the "
            + "concrete type, or annotate it with [SpecEvent]."),
    };

    /// <summary>
    /// Finds the annotated type to record an erased value as: its own type, or
    /// the abstract annotated base that makes it a variant.
    /// </summary>
    private static Type ResolveDynamicType(object value)
    {
        Type actual = value.GetType();
        for (Type? scope = actual.BaseType; scope is not null; scope = scope.BaseType)
        {
            if (scope.IsAbstract && SemanticType.HasSpecEvent(scope))
            {
                return scope;
            }
        }

        return actual;
    }

    private static Value ProjectString(string text) => Value.String(RejectUnpairedSurrogates(text));

    /// <summary>
    /// Returns the text unchanged, or refuses it when it has no UTF-8 encoding.
    /// Map keys run through this too, because a key that cannot be encoded
    /// cannot produce the same UTF-8 map the Rust runtime produces.
    /// </summary>
    private static string RejectUnpairedSurrogates(string text)
    {
        for (int index = 0; index < text.Length; index++)
        {
            if (!char.IsSurrogate(text[index]))
            {
                continue;
            }

            bool paired = char.IsHighSurrogate(text[index])
                && index + 1 < text.Length
                && char.IsLowSurrogate(text[index + 1]);
            if (!paired)
            {
                throw Rejected(
                    CaptureRejection.UnrepresentableValue,
                    string.Format(
                        CultureInfo.InvariantCulture,
                        "a string containing the unpaired surrogate U+{0:X4} at index {1} was "
                        + "observed. Such a string has no UTF-8 encoding and cannot be recorded.",
                        (int)text[index],
                        index));
            }

            index++;
        }

        return text;
    }

    private static T Require<T>(SemanticType type, object value)
    {
        if (value is T typed)
        {
            return typed;
        }

        throw Mismatch(type, value.GetType());
    }

    private static SpecGateCaptureException Mismatch(SemanticType type, Type actual) =>
        Rejected(
            CaptureRejection.TypeMismatch,
            $"a '{actual.FullName}' was observed where {Describe(type)} is declared.");

    private static SpecGateCaptureException Rejected(CaptureRejection rejection, string detail) =>
        new(rejection, "specgate: " + detail);

    private static string Describe(SemanticType type) => type.Kind switch
    {
        SemanticKind.Named => $"'{type.Declared!.FullName}'",
        SemanticKind.Option => $"an optional {Describe(type.First!)}",
        SemanticKind.List => $"a list of {Describe(type.First!)}",
        SemanticKind.Set => $"a set of {Describe(type.First!)}",
        SemanticKind.Map => $"a map of {Describe(type.Second!)}",
        SemanticKind.Result => $"a result of {Describe(type.First!)}",
        _ => $"a {type.Kind.ToString().ToUpperInvariant()} value",
    };
}
