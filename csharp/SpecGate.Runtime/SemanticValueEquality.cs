using System;
using System.Collections.Generic;
using System.Linq;

namespace SpecGate.Runtime;

/// <summary>Canonical keys for uniqueness under recursive CTSC equality.</summary>
internal static class SemanticValueEquality
{
    internal static Value Canonicalize(SemanticType type, Value value)
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
            case SemanticKind.Record:
                return Value.Map([
                    new KeyValuePair<string, Value>("key", Canonicalize(type.First!, value.AsMap()["key"])),
                    new KeyValuePair<string, Value>("items", Canonicalize(type.Second!, value.AsMap()["items"])),
                ]);
            case SemanticKind.Option:
            case SemanticKind.Result:
                KeyValuePair<string, Value> variant = value.AsMap().Single();
                if (type.Kind == SemanticKind.Option && variant.Key == "None")
                {
                    return value;
                }
                return Value.Tagged(variant.Key,
                    Canonicalize(variant.Key == "Err" ? type.Second! : type.First!, variant.Value));
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
}
