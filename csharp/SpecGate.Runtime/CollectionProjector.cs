using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using SpecGate.Collections;
using static SpecGate.Runtime.ValueProjector;

namespace SpecGate.Runtime;

/// <summary>Reads fresh generic enumerators without mutating native collections.</summary>
internal static class CollectionProjector
{
    internal static List<Value> Elements(SemanticType type, SemanticType element, object value)
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

        Type elementType = Arguments(type, value)[0];
        return Method(nameof(Sequence), [elementType])
            .CreateDelegate<Func<SemanticType, SemanticType, object, List<Value>>>()(type, element, value);
    }

    private static List<Value> Sequence<T>(SemanticType type, SemanticType element, object value)
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

    internal static Value Unique(SemanticType type, object value)
    {
        List<Value> elements = Elements(type, type.First!, value);
        if (elements.Select(element => SemanticValueEquality.Canonicalize(type.First!, element))
            .Distinct().Count() != elements.Count)
        {
            throw Rejected(CaptureRejection.UnrepresentableValue,
                "distinct native elements project to duplicate elements in a unique CTSC collection.");
        }
        return type.Kind == SemanticKind.OrderedSet ? Value.List(elements) : Value.Set(elements);
    }

    internal static Value Tuple(SemanticType type, object value)
    {
        Type actual = value.GetType();
        return Value.List([
            Project(type.First!, actual.GetField("Item1")!.GetValue(value)),
            Project(type.Second!, actual.GetField("Item2")!.GetValue(value)),
        ]);
    }

    internal static Value Map(SemanticType type, object value) =>
        Invoke(type.Projection == CollectionProjection.Lookup ? nameof(Lookup) : nameof(Entries),
            Arguments(type, value), type, value);

    internal static Value Group(SemanticType type, object value) =>
        Invoke(nameof(Grouping), Arguments(type, value), type, value);

    private static Value Entries<TKey, TValue>(SemanticType type, object value)
    {
        if (value is not IEnumerable<KeyValuePair<TKey, TValue>> dictionary)
        {
            throw Mismatch(type, value.GetType());
        }
        MapBuilder result = new(type);
        foreach (KeyValuePair<TKey, TValue> entry in dictionary)
        {
            result.Add(Project(type.First!, entry.Key), Project(type.Second!, entry.Value));
        }
        return result.Finish();
    }

    private static Value Lookup<TKey, TElement>(SemanticType type, object value)
    {
        if (value is not ILookup<TKey, TElement> lookup)
        {
            throw Mismatch(type, value.GetType());
        }
        MapBuilder result = new(type);
        foreach (IGrouping<TKey, TElement> group in lookup)
        {
            result.Add(Project(type.First!, group.Key),
                Value.List(Sequence<TElement>(type, type.Second!.First!, group)));
        }
        return result.Finish();
    }

    private static Value Grouping<TKey, TElement>(SemanticType type, object value)
    {
        if (value is not IGrouping<TKey, TElement> group)
        {
            throw Mismatch(type, value.GetType());
        }
        return Value.Map([
            new KeyValuePair<string, Value>("key", Project(type.First!, group.Key)),
            new KeyValuePair<string, Value>("items",
                Value.List(Sequence<TElement>(type, type.Second!.First!, group))),
        ]);
    }

    private static Type[] Arguments(SemanticType type, object value) =>
        type.ClrArguments ?? CollectionContractResolver.Resolve(value.GetType())!.Arguments;

    private static MethodInfo Method(string name, Type[] arguments) =>
        typeof(CollectionProjector).GetMethod(name,
            BindingFlags.Static | BindingFlags.NonPublic)!.MakeGenericMethod(arguments);

    // Delegates keep capture exceptions unwrapped, unlike MethodInfo.Invoke.
    private static Value Invoke(string name, Type[] arguments, SemanticType type, object value) =>
        Method(name, arguments).CreateDelegate<Func<SemanticType, object, Value>>()(type, value);

    private sealed class MapBuilder(SemanticType type)
    {
        private readonly List<KeyValuePair<string, Value>> entries = [];
        private readonly List<Value> orderedEntries = [];
        private readonly HashSet<Value> keys = [];
        private readonly bool compact = type.Kind == SemanticKind.Map && type.First!.Kind == SemanticKind.String;

        internal void Add(Value key, Value value)
        {
            if (!keys.Add(SemanticValueEquality.Canonicalize(type.First!, key)))
            {
                throw Rejected(CaptureRejection.UnrepresentableValue,
                    "distinct native keys project to duplicate CTSC map keys.");
            }
            if (compact)
            {
                entries.Add(new KeyValuePair<string, Value>(key.AsString(), value));
            }
            else
            {
                orderedEntries.Add(Value.Map([
                    new KeyValuePair<string, Value>("key", key),
                    new KeyValuePair<string, Value>("value", value),
                ]));
            }
        }

        internal Value Finish() => compact ? Value.Map(entries) : Value.List(orderedEntries);
    }
}
