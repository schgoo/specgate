using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Globalization;
using System.Text;

namespace SpecGate.Runtime;

/// <summary>The semantic value families CTSC records.</summary>
/// <remarks>
/// Declaration order is normative. It is the cross-family ordering rank shared
/// with the Rust runtime's <c>variant_rank</c>, so a set holding mixed families
/// sorts identically in both languages. Reordering changes captured artifacts.
/// </remarks>
public enum ValueKind
{
    /// <summary>A boolean.</summary>
    Bool,

    /// <summary>A signed 64-bit integer.</summary>
    Integer,

    /// <summary>An unsigned 64-bit integer.</summary>
    Unsigned,

    /// <summary>A 64-bit binary floating point number.</summary>
    Float,

    /// <summary>A Unicode string.</summary>
    String,

    /// <summary>An ordered sequence.</summary>
    List,

    /// <summary>A sorted, deduplicated collection.</summary>
    Set,

    /// <summary>A string-keyed mapping sorted by key.</summary>
    Map,
}

/// <summary>
/// A language-neutral semantic value, the only shape SpecGate records at an
/// operation boundary.
/// </summary>
/// <remarks>
/// <para>
/// This is the C# projection of the Rust runtime's value algebra. Equality and
/// ordering are defined so that the same observation taken in either language
/// produces the same artifact and the same comparison outcome:
/// </para>
/// <list type="bullet">
/// <item><description>floats compare by bit pattern for equality and by IEEE 754
/// total order for sorting, so <c>NaN</c> is a stable, orderable value rather
/// than an unordered one;</description></item>
/// <item><description>map keys sort by their UTF-8 byte sequence, matching Rust's
/// <c>BTreeMap&lt;String, _&gt;</c> rather than UTF-16 ordinal order;</description></item>
/// <item><description>mixed-family sets sort by <see cref="ValueKind"/> first.</description></item>
/// </list>
/// <para>
/// Instances are immutable. A value captured at a boundary is a snapshot and is
/// unaffected by later mutation of the object it was projected from.
/// </para>
/// </remarks>
public sealed class Value : IEquatable<Value>, IComparable<Value>, IComparable
{
    /// <summary>
    /// Holds <see cref="string"/>, <c>Value[]</c>, or
    /// <c>SortedDictionary&lt;string, Value&gt;</c>; <see langword="null"/> for
    /// scalar families, whose payload lives in <see cref="_scalar"/>.
    /// </summary>
    private readonly object? _payload;

    /// <summary>
    /// The scalar payload: zero or one for <see cref="ValueKind.Bool"/>, the
    /// value itself for <see cref="ValueKind.Integer"/>, a reinterpreted
    /// <see cref="ulong"/> for <see cref="ValueKind.Unsigned"/>, and the raw bits
    /// for <see cref="ValueKind.Float"/>.
    /// </summary>
    private readonly long _scalar;

    private Value(ValueKind kind, object? payload, long scalar)
    {
        Kind = kind;
        _payload = payload;
        _scalar = scalar;
    }

    /// <summary>Gets the family this value belongs to.</summary>
    public ValueKind Kind { get; }

    /// <summary>Gets the empty map, the projection of a unit value.</summary>
    public static Value Unit { get; } = Map([]);

    /// <summary>Creates a boolean value.</summary>
    /// <param name="value">The boolean.</param>
    /// <returns>The projected value.</returns>
    public static Value Bool(bool value) => new(ValueKind.Bool, null, value ? 1 : 0);

    /// <summary>Creates a signed integer value.</summary>
    /// <param name="value">The integer.</param>
    /// <returns>The projected value.</returns>
    public static Value Integer(long value) => new(ValueKind.Integer, null, value);

    /// <summary>Creates an unsigned integer value.</summary>
    /// <param name="value">The integer.</param>
    /// <returns>The projected value.</returns>
    public static Value Unsigned(ulong value) => new(ValueKind.Unsigned, null, unchecked((long)value));

    /// <summary>Creates a floating point value.</summary>
    /// <param name="value">The number.</param>
    /// <returns>The projected value.</returns>
    public static Value Float(double value) =>
        new(ValueKind.Float, null, BitConverter.DoubleToInt64Bits(value));

    /// <summary>Creates a string value.</summary>
    /// <param name="value">The string.</param>
    /// <returns>The projected value.</returns>
    /// <exception cref="ArgumentNullException"><paramref name="value"/> is null.</exception>
    public static Value String(string value)
    {
        ArgumentNullException.ThrowIfNull(value);

        return new Value(ValueKind.String, value, 0);
    }

    /// <summary>Creates a list value preserving element order.</summary>
    /// <param name="items">The elements.</param>
    /// <returns>The projected value.</returns>
    /// <exception cref="ArgumentNullException"><paramref name="items"/> or an element is null.</exception>
    public static Value List(IEnumerable<Value> items) =>
        new(ValueKind.List, Materialize(items, nameof(items)), 0);

    /// <summary>Creates a set value, sorted and deduplicated.</summary>
    /// <param name="items">The elements, in any order.</param>
    /// <returns>The projected value.</returns>
    /// <exception cref="ArgumentNullException"><paramref name="items"/> or an element is null.</exception>
    public static Value Set(IEnumerable<Value> items)
    {
        Value[] materialized = Materialize(items, nameof(items));
        Array.Sort(materialized, Compare);

        int kept = 0;
        for (int index = 0; index < materialized.Length; index++)
        {
            if (kept == 0 || Compare(materialized[kept - 1], materialized[index]) != 0)
            {
                materialized[kept] = materialized[index];
                kept++;
            }
        }

        if (kept != materialized.Length)
        {
            Array.Resize(ref materialized, kept);
        }

        return new Value(ValueKind.Set, materialized, 0);
    }

    /// <summary>Creates a map value sorted by UTF-8 key order.</summary>
    /// <param name="entries">The entries; later duplicates of a key replace earlier ones.</param>
    /// <returns>The projected value.</returns>
    /// <exception cref="ArgumentNullException"><paramref name="entries"/>, a key, or a value is null.</exception>
    public static Value Map(IEnumerable<KeyValuePair<string, Value>> entries)
    {
        ArgumentNullException.ThrowIfNull(entries);

        SortedDictionary<string, Value> sorted = new(Utf8KeyComparer.Instance);
        foreach (KeyValuePair<string, Value> entry in entries)
        {
            if (entry.Key is null)
            {
                throw new ArgumentNullException(nameof(entries), "specgate: a map key is null.");
            }

            if (entry.Value is null)
            {
                throw new ArgumentNullException(nameof(entries), "specgate: a map value is null.");
            }

            sorted[entry.Key] = entry.Value;
        }

        return new Value(ValueKind.Map, sorted, 0);
    }

    /// <summary>Creates a single-entry map, the shape used for tagged variants.</summary>
    /// <param name="key">The entry key.</param>
    /// <param name="value">The entry value.</param>
    /// <returns>The projected value.</returns>
    public static Value Tagged(string key, Value value) =>
        Map([new KeyValuePair<string, Value>(key, value)]);

    /// <summary>Gets the boolean payload.</summary>
    /// <returns>The boolean.</returns>
    /// <exception cref="InvalidOperationException">This value is not a boolean.</exception>
    public bool AsBool() => Expect(ValueKind.Bool)._scalar != 0;

    /// <summary>Gets the signed integer payload.</summary>
    /// <returns>The integer.</returns>
    /// <exception cref="InvalidOperationException">This value is not a signed integer.</exception>
    public long AsInteger() => Expect(ValueKind.Integer)._scalar;

    /// <summary>Gets the unsigned integer payload.</summary>
    /// <returns>The integer.</returns>
    /// <exception cref="InvalidOperationException">This value is not an unsigned integer.</exception>
    public ulong AsUnsigned() => unchecked((ulong)Expect(ValueKind.Unsigned)._scalar);

    /// <summary>Gets the floating point payload.</summary>
    /// <returns>The number.</returns>
    /// <exception cref="InvalidOperationException">This value is not a float.</exception>
    public double AsFloat() => BitConverter.Int64BitsToDouble(Expect(ValueKind.Float)._scalar);

    /// <summary>Gets the string payload.</summary>
    /// <returns>The string.</returns>
    /// <exception cref="InvalidOperationException">This value is not a string.</exception>
    public string AsString() => (string)Expect(ValueKind.String)._payload!;

    /// <summary>Gets the list elements in order.</summary>
    /// <returns>The elements.</returns>
    /// <exception cref="InvalidOperationException">This value is not a list.</exception>
    public IReadOnlyList<Value> AsList() => Array.AsReadOnly((Value[])Expect(ValueKind.List)._payload!);

    /// <summary>Gets the set elements in sorted order.</summary>
    /// <returns>The elements.</returns>
    /// <exception cref="InvalidOperationException">This value is not a set.</exception>
    public IReadOnlyList<Value> AsSet() => Array.AsReadOnly((Value[])Expect(ValueKind.Set)._payload!);

    /// <summary>Gets the map entries, which enumerate in sorted key order.</summary>
    /// <returns>The entries.</returns>
    /// <exception cref="InvalidOperationException">This value is not a map.</exception>
    public IReadOnlyDictionary<string, Value> AsMap() =>
        new ReadOnlyDictionary<string, Value>((SortedDictionary<string, Value>)Expect(ValueKind.Map)._payload!);

    /// <summary>Compares two values in CTSC order.</summary>
    /// <param name="left">The left value, which may be null.</param>
    /// <param name="right">The right value, which may be null.</param>
    /// <returns>A negative number, zero, or a positive number.</returns>
    public static int Compare(Value? left, Value? right)
    {
        if (ReferenceEquals(left, right))
        {
            return 0;
        }

        if (left is null)
        {
            return -1;
        }

        if (right is null)
        {
            return 1;
        }

        if (left.Kind != right.Kind)
        {
            return ((int)left.Kind).CompareTo((int)right.Kind);
        }

        switch (left.Kind)
        {
            case ValueKind.Bool:
            case ValueKind.Integer:
                return left._scalar.CompareTo(right._scalar);
            case ValueKind.Unsigned:
                return left.AsUnsigned().CompareTo(right.AsUnsigned());
            case ValueKind.Float:
                return CompareFloatBits(left._scalar, right._scalar);
            case ValueKind.String:
                return Utf8KeyComparer.Instance.Compare((string)left._payload!, (string)right._payload!);
            case ValueKind.List:
            case ValueKind.Set:
                return CompareSequences((Value[])left._payload!, (Value[])right._payload!);
            default:
                return CompareMaps(
                    (SortedDictionary<string, Value>)left._payload!,
                    (SortedDictionary<string, Value>)right._payload!);
        }
    }

    /// <inheritdoc/>
    public int CompareTo(Value? other) => Compare(this, other);

    /// <inheritdoc/>
    public int CompareTo(object? obj) => obj switch
    {
        null => 1,
        Value other => Compare(this, other),
        _ => throw new ArgumentException("specgate: can only compare against another Value.", nameof(obj)),
    };

    /// <inheritdoc/>
    public bool Equals(Value? other) => Compare(this, other) == 0;

    /// <inheritdoc/>
    public override bool Equals(object? obj) => obj is Value other && Compare(this, other) == 0;

    /// <inheritdoc/>
    public override int GetHashCode()
    {
        unchecked
        {
            int hash = (int)Kind * 397;
            switch (Kind)
            {
                case ValueKind.String:
                    return (hash * 31) + ((string)_payload!).GetHashCode();
                case ValueKind.List:
                case ValueKind.Set:
                    foreach (Value item in (Value[])_payload!)
                    {
                        hash = (hash * 31) + item.GetHashCode();
                    }

                    return hash;
                case ValueKind.Map:
                    foreach (KeyValuePair<string, Value> entry in (SortedDictionary<string, Value>)_payload!)
                    {
                        hash = (hash * 31) + entry.Key.GetHashCode();
                        hash = (hash * 31) + entry.Value.GetHashCode();
                    }

                    return hash;
                default:
                    return (hash * 31) + _scalar.GetHashCode();
            }
        }
    }

    /// <summary>Tests two values for CTSC equality.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>Whether the values are equal.</returns>
    public static bool operator ==(Value? left, Value? right) => Compare(left, right) == 0;

    /// <summary>Tests two values for CTSC inequality.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>Whether the values differ.</returns>
    public static bool operator !=(Value? left, Value? right) => Compare(left, right) != 0;

    /// <summary>Tests whether the left value sorts before the right.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>Whether the left value sorts first.</returns>
    public static bool operator <(Value? left, Value? right) => Compare(left, right) < 0;

    /// <summary>Tests whether the left value sorts before or equal to the right.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>Whether the left value does not sort after.</returns>
    public static bool operator <=(Value? left, Value? right) => Compare(left, right) <= 0;

    /// <summary>Tests whether the left value sorts after the right.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>Whether the left value sorts last.</returns>
    public static bool operator >(Value? left, Value? right) => Compare(left, right) > 0;

    /// <summary>Tests whether the left value sorts after or equal to the right.</summary>
    /// <param name="left">The left value.</param>
    /// <param name="right">The right value.</param>
    /// <returns>Whether the left value does not sort before.</returns>
    public static bool operator >=(Value? left, Value? right) => Compare(left, right) >= 0;

    /// <summary>Renders the value for diagnostics, mirroring the Rust runtime's rendering.</summary>
    /// <remarks>
    /// Lists and sets both render as <c>[a,b]</c>, maps as <c>{"k":v}</c>, and
    /// strings are quoted inside a container but not at the top level, matching
    /// <c>Display for Value</c> in the Rust runtime. Numeric text is .NET's, so
    /// non-finite floats still print differently from Rust's <c>inf</c>.
    /// </remarks>
    /// <returns>The rendered value.</returns>
    public override string ToString()
    {
        StringBuilder builder = new();
        Render(builder, quoteStrings: false);
        return builder.ToString();
    }

    private void Render(StringBuilder builder, bool quoteStrings)
    {
        switch (Kind)
        {
            case ValueKind.Bool:
                builder.Append(AsBool() ? "true" : "false");
                break;
            case ValueKind.Integer:
                builder.Append(_scalar.ToString(CultureInfo.InvariantCulture));
                break;
            case ValueKind.Unsigned:
                builder.Append(AsUnsigned().ToString(CultureInfo.InvariantCulture));
                break;
            case ValueKind.Float:
                builder.Append(AsFloat().ToString("R", CultureInfo.InvariantCulture));
                break;
            case ValueKind.String:
                if (quoteStrings)
                {
                    builder.Append('"').Append((string)_payload!).Append('"');
                }
                else
                {
                    builder.Append((string)_payload!);
                }

                break;
            case ValueKind.List:
            case ValueKind.Set:
                builder.Append('[');
                RenderSequence(builder, (Value[])_payload!);
                builder.Append(']');
                break;
            default:
                builder.Append('{');
                bool first = true;
                foreach (KeyValuePair<string, Value> entry in (SortedDictionary<string, Value>)_payload!)
                {
                    if (!first)
                    {
                        builder.Append(',');
                    }

                    first = false;
                    builder.Append('"').Append(entry.Key).Append("\":");
                    entry.Value.Render(builder, quoteStrings: true);
                }

                builder.Append('}');
                break;
        }
    }

    private static void RenderSequence(StringBuilder builder, Value[] items)
    {
        for (int index = 0; index < items.Length; index++)
        {
            if (index > 0)
            {
                builder.Append(',');
            }

            items[index].Render(builder, quoteStrings: true);
        }
    }

    /// <summary>
    /// Orders two floats by IEEE 754 total order, matching Rust's
    /// <c>f64::total_cmp</c>.
    /// </summary>
    /// <remarks>
    /// The sign bit is broadcast and shifted to flip the magnitude ordering of
    /// negative values, which turns the raw bit patterns into a monotonically
    /// increasing sequence. <see cref="double.CompareTo(double)"/> cannot be used:
    /// it collapses every <c>NaN</c> to one value and sorts it below negative
    /// infinity, so captures would not round-trip against Rust.
    /// </remarks>
    private static int CompareFloatBits(long left, long right)
    {
        left ^= unchecked((long)((ulong)(left >> 63) >> 1));
        right ^= unchecked((long)((ulong)(right >> 63) >> 1));
        return left.CompareTo(right);
    }

    private static int CompareSequences(Value[] left, Value[] right)
    {
        int shared = Math.Min(left.Length, right.Length);
        for (int index = 0; index < shared; index++)
        {
            int order = Compare(left[index], right[index]);
            if (order != 0)
            {
                return order;
            }
        }

        return left.Length.CompareTo(right.Length);
    }

    private static int CompareMaps(
        SortedDictionary<string, Value> left,
        SortedDictionary<string, Value> right)
    {
        SortedDictionary<string, Value>.Enumerator leftEntries = left.GetEnumerator();
        SortedDictionary<string, Value>.Enumerator rightEntries = right.GetEnumerator();
        try
        {
            while (leftEntries.MoveNext())
            {
                if (!rightEntries.MoveNext())
                {
                    return 1;
                }

                int order = Utf8KeyComparer.Instance.Compare(
                    leftEntries.Current.Key, rightEntries.Current.Key);
                if (order != 0)
                {
                    return order;
                }

                order = Compare(leftEntries.Current.Value, rightEntries.Current.Value);
                if (order != 0)
                {
                    return order;
                }
            }

            return rightEntries.MoveNext() ? -1 : 0;
        }
        finally
        {
            leftEntries.Dispose();
            rightEntries.Dispose();
        }
    }

    private static Value[] Materialize(IEnumerable<Value> items, string parameterName)
    {
        if (items is null)
        {
            throw new ArgumentNullException(parameterName);
        }

        List<Value> collected = [];
        foreach (Value item in items)
        {
            if (item is null)
            {
                throw new ArgumentNullException(parameterName, "specgate: a collection element is null.");
            }

            collected.Add(item);
        }

        return [.. collected];
    }

    private Value Expect(ValueKind kind)
    {
        if (Kind != kind)
        {
            throw new InvalidOperationException(
                $"specgate: expected a {kind} value but this value is a {Kind}.");
        }

        return this;
    }
}
