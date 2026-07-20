using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Reflection;
using System.Text;

namespace SpecGate.Runtime;

/// <summary>
/// Provides the trace-emission primitives used by generated SpecGate C# runners
/// and by fixtures that explicitly emit observable events.
/// </summary>
/// <remarks>
/// The runtime stores trace records in thread-local state so each spec case can
/// be reset and collected independently. Values are serialized into SpecGate's
/// deterministic JSON trace representation, including annotated structured
/// event objects, option/result shims, lists, sets, and maps.
/// </remarks>
public static class SpecGateRuntime
{
    [ThreadStatic]
    private static List<string>? _events;

    [ThreadStatic]
    private static Dictionary<object, string?>? _objectPrefixes;

    [ThreadStatic]
    private static Dictionary<string, Dictionary<string, string>>? _mocks;

    private static List<string> Events => _events ??= [];

    private static Dictionary<object, string?> ObjectPrefixes => _objectPrefixes ??= new Dictionary<object, string?>(ReferenceComparer.Instance);

    private static Dictionary<string, Dictionary<string, string>> Mocks => _mocks ??= [];

    /// <summary>
    /// Clears the current thread's accumulated trace records and registered
    /// object prefixes before starting a new spec case.
    /// </summary>
    public static void Reset()
    {
        Events.Clear();
        ObjectPrefixes.Clear();
        Mocks.Clear();
    }

    /// <summary>
    /// Emits a SpecGate <c>Run</c> trace record for the operation currently being
    /// invoked.
    /// </summary>
    /// <param name="operation">The spec operation name being executed.</param>
    public static void EmitRun(string operation)
    {
        Events.Add("{\"kind\":\"Run\",\"operation\":" + QuoteJson(operation) + "}");
    }

    /// <summary>
    /// Emits a string-valued SpecGate event trace record.
    /// </summary>
    /// <param name="name">The event name expected by the spec.</param>
    /// <param name="value">The string value to serialize.</param>
    public static void EmitEvent(string name, string value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + QuoteJson(value) + "}");
    }

    /// <summary>
    /// Emits an integer-valued SpecGate event trace record.
    /// </summary>
    /// <param name="name">The event name expected by the spec.</param>
    /// <param name="value">The integer value to serialize.</param>
    public static void EmitEvent(string name, int value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + value.ToString(CultureInfo.InvariantCulture) + "}");
    }

    /// <summary>
    /// Emits a Boolean-valued SpecGate event trace record.
    /// </summary>
    /// <param name="name">The event name expected by the spec.</param>
    /// <param name="value">The Boolean value to serialize.</param>
    public static void EmitEvent(string name, bool value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + (value ? "true" : "false") + "}");
    }

    /// <summary>
    /// Emits a SpecGate event trace record for an arbitrary value.
    /// </summary>
    /// <param name="name">The event name expected by the spec.</param>
    /// <param name="value">
    /// The value to serialize using SpecGate's deterministic value conversion.
    /// Annotated event types are serialized as structured maps.
    /// </param>
    public static void EmitEvent(string name, object? value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + ToSpecValue(value).ToJson() + "}");
    }

    /// <summary>
    /// Emits the return value of an operation as a <c>$result</c> event.
    /// </summary>
    /// <typeparam name="T">The static return type of the operation.</typeparam>
    /// <param name="value">The operation return value.</param>
    /// <remarks>
    /// Nullable value types are represented as SpecGate option values. Other
    /// values are delegated to the object overload so option/result shims and
    /// annotated structured values keep their canonical trace shape.
    /// </remarks>
    public static void EmitResult<T>(T value)
    {
        var nullableInner = Nullable.GetUnderlyingType(typeof(T));
        if (nullableInner is not null)
        {
            EmitEvent("$result", TaggedMap(value is null ? "None" : "Some", value is null ? EmptyMap() : ToSpecValue(value)));
            return;
        }

        EmitResult((object?)value);
    }

    /// <summary>
    /// Emits the return value of an operation as a <c>$result</c> event.
    /// </summary>
    /// <param name="value">The operation return value, or <see langword="null"/>.</param>
    /// <remarks>
    /// Annotated non-variant event objects emit their annotated members as
    /// top-level events, matching the Rust <c>SpecEvent</c> behavior. Variant,
    /// option, and result values are emitted as structured <c>$result</c> maps.
    /// </remarks>
    public static void EmitResult(object? value)
    {
        if (value is null)
        {
            EmitEvent("$result", TaggedMap("None", EmptyMap()));
            return;
        }

        if (TryOptionToSpecValue(value, out var optionValue) || TryResultToSpecValue(value, out optionValue))
        {
            EmitEvent("$result", optionValue);
            return;
        }

        EmitResultObject(value);
    }

    /// <summary>
    /// Registers an object with an optional event-name prefix for subsequent
    /// state capture.
    /// </summary>
    /// <param name="value">The object whose annotated members may be emitted.</param>
    /// <param name="prefix">
    /// The prefix to prepend to emitted member names, or <see langword="null"/>
    /// to emit bare member names.
    /// </param>
    /// <remarks>
    /// Generated stateful runners use this to distinguish multiple setup objects
    /// of the same type, for example <c>source.balance</c> and
    /// <c>target.balance</c>.
    /// </remarks>
    public static void RegisterObject(object? value, string? prefix)
    {
        if (value is not null)
        {
            ObjectPrefixes[value] = prefix;
        }
    }

    /// <summary>
    /// Emits a registered object's member value using that object's configured
    /// prefix.
    /// </summary>
    /// <param name="owner">The object instance that owns the member.</param>
    /// <param name="name">The annotated member event name.</param>
    /// <param name="value">The new member value to serialize.</param>
    /// <remarks>
    /// If <paramref name="owner"/> has not been registered, no event is emitted.
    /// This allows instrumented fixture copies to remain quiet outside harnessed
    /// setup objects.
    /// </remarks>
    public static void EmitMember(object? owner, string name, object? value)
    {
        if (owner is null || !ObjectPrefixes.TryGetValue(owner, out var prefix))
        {
            return;
        }

        EmitEvent(prefix is null ? name : prefix + "." + name, value);
    }

    /// <summary>
    /// Emits every annotated public field or property on an object.
    /// </summary>
    /// <param name="value">The object whose annotated members should be captured.</param>
    /// <param name="prefix">
    /// An optional prefix to prepend to each member event name, such as an
    /// operation parameter name.
    /// </param>
    public static void EmitFields(object? value, string? prefix = null)
    {
        if (value is null)
        {
            return;
        }

        foreach (var member in SpecEventMembers(value.GetType()).OrderBy(m => m.Token))
        {
            var name = prefix is null ? member.EventName : prefix + "." + member.EventName;
            EmitEvent(name, member.GetValue(value));
        }
    }

    private static void EmitResultObject(object value)
    {
        if (value is not null && IsSpecEventType(value.GetType()))
        {
            if (IsSpecVariantType(value.GetType()))
            {
                EmitEvent("$result", value);
                return;
            }

            foreach (var member in SpecEventMembers(value.GetType()).OrderBy(m => m.Token))
            {
                EmitEvent(member.EventName, member.GetValue(value));
            }
        }

        EmitEvent("$result", value);
    }

    /// <summary>
    /// Gets the current thread's accumulated trace records as a JSON array.
    /// </summary>
    /// <returns>
    /// A JSON array string containing the trace records emitted since the last
    /// call to <see cref="Reset"/>.
    /// </returns>
    public static string GetTracesJson()
    {
        var sb = new StringBuilder("[");
        for (int i = 0; i < Events.Count; i++)
        {
            if (i > 0) sb.Append(',');
            sb.Append(Events[i]);
        }
        sb.Append(']');
        return sb.ToString();
    }

    internal static void SetMock(string name, IDictionary<string, string> entries)
    {
        Mocks[name] = new Dictionary<string, string>(entries, StringComparer.Ordinal);
    }

    internal static T MockCall<T>(string name, object? input, out bool hit)
    {
        string key = CanonicalMockKey(input);
        EmitEvent(name + ".request", key);
        if (Mocks.TryGetValue(name, out var table) && table.TryGetValue(key, out string? response))
        {
            hit = true;
            EmitEvent(name + ".response", response);
            return FromMockResponse<T>(response);
        }

        hit = false;
        EmitEvent(name + ".error", "no mock response for input '" + key + "'");
        return SpecDefault<T>();
    }

    internal static T SpecDefault<T>()
    {
        if (typeof(T) == typeof(string))
        {
            return (T)(object)string.Empty;
        }

        return default!;
    }

    private static string CanonicalMockKey(object? input)
    {
        return input is string s ? s : ToSpecValue(input).ToJson();
    }

    private static T FromMockResponse<T>(string response)
    {
        if (typeof(T) == typeof(string))
        {
            return (T)(object)response;
        }

        return (T)Convert.ChangeType(response, typeof(T), CultureInfo.InvariantCulture);
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
        if (TryOptionToSpecValue(value, out var optionValue) || TryResultToSpecValue(value, out optionValue))
        {
            return optionValue;
        }

        if (IsSpecVariantType(type))
        {
            return SpecVariantToSpecValue(value, type);
        }

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

            return new SetValue([.. set]);
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

    private static MapValue SpecVariantToSpecValue(object value, Type type)
    {
        var payload = new SortedDictionary<string, SpecValue>(Utf8StringComparer.Instance);
        foreach (var member in SpecEventMembers(type))
        {
            payload[member.EventName] = ToSpecValue(member.GetValue(value));
        }

        return TaggedMap(SpecEventName(type), new MapValue(payload));
    }

    private static MapValue TaggedMap(string tag, SpecValue payload)
    {
        var map = new SortedDictionary<string, SpecValue>(Utf8StringComparer.Instance)
        {
            [tag] = payload,
        };
        return new MapValue(map);
    }

    private static MapValue EmptyMap() => new(new SortedDictionary<string, SpecValue>(Utf8StringComparer.Instance));

    private static bool TryOptionToSpecValue(object value, out SpecValue specValue)
    {
        var type = value.GetType();
        if (!type.IsGenericType || type.GetGenericTypeDefinition() != typeof(Option<>))
        {
            specValue = EmptyMap();
            return false;
        }

        bool hasValue = (bool)(type.GetProperty(nameof(Option<int>.HasValue))?.GetValue(value) ?? false);
        specValue = hasValue
            ? TaggedMap("Some", ToSpecValue(type.GetProperty(nameof(Option<int>.Value))?.GetValue(value)))
            : TaggedMap("None", EmptyMap());
        return true;
    }

    private static bool TryResultToSpecValue(object value, out SpecValue specValue)
    {
        var type = value.GetType();
        if (!type.IsGenericType || type.GetGenericTypeDefinition() != typeof(Result<,>))
        {
            specValue = EmptyMap();
            return false;
        }

        bool isOk = (bool)(type.GetProperty(nameof(Result<int, string>.IsOk))?.GetValue(value) ?? false);
        specValue = isOk
            ? TaggedMap("Ok", ToSpecValue(type.GetProperty(nameof(Result<int, string>.OkValue))?.GetValue(value)))
            : TaggedMap("Err", ToSpecValue(type.GetProperty(nameof(Result<int, string>.ErrValue))?.GetValue(value)));
        return true;
    }

    private static MapValue MapFromDictionary(IDictionary dict)
    {
        var map = new SortedDictionary<string, SpecValue>(Utf8StringComparer.Instance);
        foreach (DictionaryEntry entry in dict)
        {
            map[Convert.ToString(entry.Key, CultureInfo.InvariantCulture) ?? string.Empty] = ToSpecValue(entry.Value);
        }

        return new MapValue(map);
    }

    private static MapValue MapFromKeyValueEnumerable(IEnumerable enumerable)
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
            .Concat([type])
            .Any(i => i.IsGenericType
                && i.GetGenericTypeDefinition() == typeof(IEnumerable<>)
                && i.GetGenericArguments()[0].IsGenericType
                && i.GetGenericArguments()[0].GetGenericTypeDefinition() == typeof(KeyValuePair<,>)
                && i.GetGenericArguments()[0].GetGenericArguments()[0] == typeof(string));

    private static bool IsSet(Type type) =>
        type.GetInterfaces()
            .Concat([type])
            .Any(i => i.IsGenericType && i.GetGenericTypeDefinition() == typeof(ISet<>));

    private static bool IsSpecEventType(Type type) =>
        SpecEventAttribute(type) is not null || SpecEventMembers(type).Any();

    private static bool IsSpecVariantType(Type type) =>
        type.BaseType is not null
        && type.BaseType.IsAbstract
        && IsSpecEventType(type.BaseType);

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

    private static string SpecEventName(Type type)
    {
        var attr = SpecEventAttribute(type);
        return attr is null ? type.Name : EventName(type.Name, attr);
    }

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

    private sealed class ReferenceComparer : IEqualityComparer<object>
    {
        public static readonly ReferenceComparer Instance = new();

        public new bool Equals(object? x, object? y) => ReferenceEquals(x, y);

        public int GetHashCode(object obj) => System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(obj);
    }
}

/// <summary>
/// Represents an optional SpecGate value for C# fixtures.
/// </summary>
/// <typeparam name="T">The type of the contained value when the option is present.</typeparam>
/// <remarks>
/// The runtime serializes <see cref="Option{T}"/> as a SpecGate tagged map with
/// either <c>Some</c> containing the converted value or <c>None</c> containing
/// an empty map. This mirrors the Rust option trace shape used for cross-target
/// comparison.
/// </remarks>
public readonly struct Option<T>
{
    private readonly T? _value;

    private Option(T? value, bool hasValue)
    {
        _value = value;
        HasValue = hasValue;
    }

    /// <summary>
    /// Gets a value indicating whether this option contains a value.
    /// </summary>
    public bool HasValue { get; }

    /// <summary>
    /// Gets the contained value when <see cref="HasValue"/> is <see langword="true"/>.
    /// </summary>
    /// <exception cref="InvalidOperationException">
    /// Thrown when the option is <see cref="None"/>.
    /// </exception>
    public T? Value => HasValue ? _value : throw new InvalidOperationException("Option has no value");

    /// <summary>
    /// Creates an option containing a value.
    /// </summary>
    /// <param name="value">The value to include in the option.</param>
    /// <returns>An option represented in traces as <c>Some</c>.</returns>
    public static Option<T> Some(T value) => new(value, true);

    /// <summary>
    /// Creates an option with no value.
    /// </summary>
    /// <returns>An option represented in traces as <c>None</c>.</returns>
    public static Option<T> None() => new(default, false);
}

/// <summary>
/// Represents a SpecGate result value for C# fixtures.
/// </summary>
/// <typeparam name="T">The type of the successful value.</typeparam>
/// <typeparam name="E">The type of the error value.</typeparam>
/// <remarks>
/// The runtime serializes <see cref="Result{T, E}"/> as a SpecGate tagged map
/// with either <c>Ok</c> or <c>Err</c>, matching the Rust result trace shape.
/// </remarks>
public readonly struct Result<T, E>
{
    private readonly T? _ok;
    private readonly E? _err;

    private Result(T? ok, E? err, bool isOk)
    {
        _ok = ok;
        _err = err;
        IsOk = isOk;
    }

    /// <summary>
    /// Gets a value indicating whether this result is an <c>Ok</c> value.
    /// </summary>
    public bool IsOk { get; }

    /// <summary>
    /// Gets the successful value when <see cref="IsOk"/> is <see langword="true"/>.
    /// </summary>
    /// <exception cref="InvalidOperationException">
    /// Thrown when the result is an <c>Err</c> value.
    /// </exception>
    public T? OkValue => IsOk ? _ok : throw new InvalidOperationException("Result is Err");

    /// <summary>
    /// Gets the error value when <see cref="IsOk"/> is <see langword="false"/>.
    /// </summary>
    /// <exception cref="InvalidOperationException">
    /// Thrown when the result is an <c>Ok</c> value.
    /// </exception>
    public E? ErrValue => !IsOk ? _err : throw new InvalidOperationException("Result is Ok");

    /// <summary>
    /// Creates a successful result.
    /// </summary>
    /// <param name="value">The success value to include in the result.</param>
    /// <returns>A result represented in traces as <c>Ok</c>.</returns>
    public static Result<T, E> Ok(T value) => new(value, default, true);

    /// <summary>
    /// Creates an error result.
    /// </summary>
    /// <param name="error">The error value to include in the result.</param>
    /// <returns>A result represented in traces as <c>Err</c>.</returns>
    public static Result<T, E> Err(E error) => new(default, error, false);
}
