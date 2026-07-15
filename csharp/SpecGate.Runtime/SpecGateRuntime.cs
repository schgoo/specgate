using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Reflection;
using System.Text;

namespace SpecGate.Runtime;

public static class SpecGateRuntime
{
    [ThreadStatic]
    private static List<string>? _events;

    private static List<string> Events => _events ??= new List<string>();

    public static void Reset() => Events.Clear();

    public static void EmitRun(string operation)
    {
        Events.Add("{\"kind\":\"Run\",\"operation\":" + QuoteJson(operation) + "}");
    }

    public static void EmitEvent(string name, string value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + QuoteJson(value) + "}");
    }

    public static void EmitEvent(string name, int value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + value.ToString(System.Globalization.CultureInfo.InvariantCulture) + "}");
    }

    public static void EmitEvent(string name, bool value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + (value ? "true" : "false") + "}");
    }

    public static void EmitEvent(string name, object? value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + ToSpecValue(value).ToJson() + "}");
    }

    public static void EmitResult(object? value)
    {
        if (value is not null && IsSpecEventType(value.GetType()))
        {
            foreach (var member in SpecEventMembers(value.GetType()).OrderBy(m => m.Token))
            {
                EmitEvent(member.EventName, member.GetValue(value));
            }
        }

        EmitEvent("$result", value);
    }

    public static string GetTracesJson()
    {
        var sb = new System.Text.StringBuilder("[");
        for (int i = 0; i < Events.Count; i++)
        {
            if (i > 0) sb.Append(',');
            sb.Append(Events[i]);
        }
        sb.Append(']');
        return sb.ToString();
    }

    private static string QuoteJson(string s)
    {
        var sb = new StringBuilder("\"");
        foreach (char c in s)
        {
            switch (c)
            {
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                case '\t': sb.Append("\\t"); break;
                default:
                    if (c < 0x20) sb.Append($"\\u{(int)c:x4}");
                    else sb.Append(c);
                    break;
            }
        }
        sb.Append('"');
        return sb.ToString();
    }

    private static SpecValue ToSpecValue(object? value)
    {
        if (value is null)
        {
            return new StringValue(string.Empty);
        }

        switch (value)
        {
            case SpecValue specValue:
                return specValue;
            case string s:
                return new StringValue(s);
            case char c:
                return new StringValue(c.ToString());
            case bool b:
                return new BoolValue(b);
            case byte or sbyte or short or ushort or int or uint or long or ulong:
                return new IntegerValue(Convert.ToInt64(value, CultureInfo.InvariantCulture));
            case float or double or decimal:
                return new FloatValue(Convert.ToDouble(value, CultureInfo.InvariantCulture));
            case IDictionary dict:
                return MapFromDictionary(dict);
        }

        var type = value.GetType();
        if (IsGenericKeyValueEnumerable(type))
        {
            return MapFromKeyValueEnumerable((IEnumerable)value);
        }

        if (IsSet(type))
        {
            var set = new SortedSet<SpecValue>();
            foreach (var item in (IEnumerable)value)
            {
                set.Add(ToSpecValue(item));
            }

            return new SetValue(set.ToList());
        }

        if (value is IEnumerable enumerable)
        {
            var items = new List<SpecValue>();
            foreach (var item in enumerable)
            {
                items.Add(ToSpecValue(item));
            }

            return new ListValue(items);
        }

        if (IsSpecEventType(type))
        {
            var map = new SortedDictionary<string, SpecValue>(Utf8StringComparer.Instance);
            foreach (var member in SpecEventMembers(type))
            {
                map[member.EventName] = ToSpecValue(member.GetValue(value));
            }

            return new MapValue(map);
        }

        return new StringValue(Convert.ToString(value, CultureInfo.InvariantCulture) ?? string.Empty);
    }

    private static SpecValue MapFromDictionary(IDictionary dict)
    {
        var map = new SortedDictionary<string, SpecValue>(Utf8StringComparer.Instance);
        foreach (DictionaryEntry entry in dict)
        {
            map[Convert.ToString(entry.Key, CultureInfo.InvariantCulture) ?? string.Empty] = ToSpecValue(entry.Value);
        }

        return new MapValue(map);
    }

    private static SpecValue MapFromKeyValueEnumerable(IEnumerable enumerable)
    {
        var map = new SortedDictionary<string, SpecValue>(Utf8StringComparer.Instance);
        foreach (var item in enumerable)
        {
            var itemType = item.GetType();
            var key = itemType.GetProperty("Key")?.GetValue(item);
            var value = itemType.GetProperty("Value")?.GetValue(item);
            map[Convert.ToString(key, CultureInfo.InvariantCulture) ?? string.Empty] = ToSpecValue(value);
        }

        return new MapValue(map);
    }

    private static bool IsGenericKeyValueEnumerable(Type type) =>
        type.GetInterfaces()
            .Concat(new[] { type })
            .Any(i => i.IsGenericType
                && i.GetGenericTypeDefinition() == typeof(IEnumerable<>)
                && i.GetGenericArguments()[0].IsGenericType
                && i.GetGenericArguments()[0].GetGenericTypeDefinition() == typeof(KeyValuePair<,>)
                && i.GetGenericArguments()[0].GetGenericArguments()[0] == typeof(string));

    private static bool IsSet(Type type) =>
        type.GetInterfaces()
            .Concat(new[] { type })
            .Any(i => i.IsGenericType && i.GetGenericTypeDefinition() == typeof(ISet<>));

    private static bool IsSpecEventType(Type type) =>
        type.GetCustomAttributes(false).Any(a => a.GetType().FullName == "SpecGate.Annotations.SpecEventAttribute");

    private static IEnumerable<EventMember> SpecEventMembers(Type type)
    {
        const BindingFlags flags = BindingFlags.Instance | BindingFlags.Public;
        foreach (var field in type.GetFields(flags))
        {
            var attr = SpecEventAttribute(field);
            if (attr is not null)
            {
                yield return new EventMember(EventName(field.Name, attr), field.MetadataToken, obj => field.GetValue(obj));
            }
        }

        foreach (var prop in type.GetProperties(flags))
        {
            if (prop.GetIndexParameters().Length != 0)
            {
                continue;
            }

            var attr = SpecEventAttribute(prop);
            if (attr is not null)
            {
                yield return new EventMember(EventName(prop.Name, attr), prop.MetadataToken, obj => prop.GetValue(obj, null));
            }
        }
    }

    private static Attribute? SpecEventAttribute(MemberInfo member) =>
        member.GetCustomAttributes(false)
            .OfType<Attribute>()
            .FirstOrDefault(a => a.GetType().FullName == "SpecGate.Annotations.SpecEventAttribute");

    private static string EventName(string fallback, Attribute attr)
    {
        var name = attr.GetType().GetProperty("Name")?.GetValue(attr) as string;
        return string.IsNullOrEmpty(name) ? fallback : name!;
    }

    private sealed class EventMember
    {
        public EventMember(string eventName, int token, Func<object, object?> getValue)
        {
            EventName = eventName;
            Token = token;
            GetValue = getValue;
        }

        public string EventName { get; }
        public int Token { get; }
        public Func<object, object?> GetValue { get; }
    }

    private abstract class SpecValue : IComparable<SpecValue>
    {
        public abstract int Rank { get; }
        public abstract string ToJson();
        protected abstract int CompareSame(SpecValue other);

        public int CompareTo(SpecValue? other)
        {
            if (other is null) return 1;
            int rank = Rank.CompareTo(other.Rank);
            return rank != 0 ? rank : CompareSame(other);
        }
    }

    private sealed class BoolValue : SpecValue
    {
        private readonly bool _value;
        public BoolValue(bool value) => _value = value;
        public override int Rank => 0;
        public override string ToJson() => _value ? "true" : "false";
        protected override int CompareSame(SpecValue other) => _value.CompareTo(((BoolValue)other)._value);
    }

    private sealed class IntegerValue : SpecValue
    {
        private readonly long _value;
        public IntegerValue(long value) => _value = value;
        public override int Rank => 1;
        public override string ToJson() => _value.ToString(CultureInfo.InvariantCulture);
        protected override int CompareSame(SpecValue other) => _value.CompareTo(((IntegerValue)other)._value);
    }

    private sealed class FloatValue : SpecValue
    {
        private readonly double _value;
        public FloatValue(double value) => _value = value;
        public override int Rank => 2;
        public override string ToJson() => _value.ToString("R", CultureInfo.InvariantCulture);
        protected override int CompareSame(SpecValue other) => _value.CompareTo(((FloatValue)other)._value);
    }

    private sealed class StringValue : SpecValue
    {
        private readonly string _value;
        public StringValue(string value) => _value = value;
        public override int Rank => 3;
        public override string ToJson() => QuoteJson(_value);
        protected override int CompareSame(SpecValue other) => Utf8StringComparer.Instance.Compare(_value, ((StringValue)other)._value);
    }

    private sealed class ListValue : SpecValue
    {
        private readonly IReadOnlyList<SpecValue> _items;
        public ListValue(IReadOnlyList<SpecValue> items) => _items = items;
        public override int Rank => 4;
        public override string ToJson() => "[" + string.Join(",", _items.Select(i => i.ToJson())) + "]";
        protected override int CompareSame(SpecValue other) => CompareLists(_items, ((ListValue)other)._items);
    }

    private sealed class SetValue : SpecValue
    {
        private readonly IReadOnlyList<SpecValue> _items;
        public SetValue(IReadOnlyList<SpecValue> items) => _items = items;
        public override int Rank => 5;
        public override string ToJson() => "[" + string.Join(",", _items.Select(i => i.ToJson())) + "]";
        protected override int CompareSame(SpecValue other) => CompareLists(_items, ((SetValue)other)._items);
    }

    private sealed class MapValue : SpecValue
    {
        private readonly SortedDictionary<string, SpecValue> _map;
        public MapValue(SortedDictionary<string, SpecValue> map) => _map = map;
        public override int Rank => 6;

        public override string ToJson() =>
            "{" + string.Join(",", _map.Select(kv => QuoteJson(kv.Key) + ":" + kv.Value.ToJson())) + "}";

        protected override int CompareSame(SpecValue other)
        {
            var rhs = ((MapValue)other)._map;
            using var left = _map.GetEnumerator();
            using var right = rhs.GetEnumerator();
            while (true)
            {
                bool hasLeft = left.MoveNext();
                bool hasRight = right.MoveNext();
                if (!hasLeft || !hasRight)
                {
                    return hasLeft.CompareTo(hasRight);
                }

                int keyCmp = Utf8StringComparer.Instance.Compare(left.Current.Key, right.Current.Key);
                if (keyCmp != 0) return keyCmp;
                int valueCmp = left.Current.Value.CompareTo(right.Current.Value);
                if (valueCmp != 0) return valueCmp;
            }
        }
    }

    private static int CompareLists(IReadOnlyList<SpecValue> left, IReadOnlyList<SpecValue> right)
    {
        int count = Math.Min(left.Count, right.Count);
        for (int i = 0; i < count; i++)
        {
            int cmp = left[i].CompareTo(right[i]);
            if (cmp != 0) return cmp;
        }

        return left.Count.CompareTo(right.Count);
    }

    private sealed class Utf8StringComparer : IComparer<string>
    {
        public static readonly Utf8StringComparer Instance = new();

        public int Compare(string? x, string? y)
        {
            if (ReferenceEquals(x, y)) return 0;
            if (x is null) return -1;
            if (y is null) return 1;
            byte[] xb = Encoding.UTF8.GetBytes(x);
            byte[] yb = Encoding.UTF8.GetBytes(y);
            int len = Math.Min(xb.Length, yb.Length);
            for (int i = 0; i < len; i++)
            {
                int cmp = xb[i].CompareTo(yb[i]);
                if (cmp != 0) return cmp;
            }

            return xb.Length.CompareTo(yb.Length);
        }
    }
}