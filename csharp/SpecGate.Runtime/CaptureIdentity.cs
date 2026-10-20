using System;

namespace SpecGate.Runtime;

/// <summary>
/// A validated lowercase hexadecimal capture protocol identifier.
/// </summary>
/// <remarks>
/// OTLP identifiers are fixed-width byte strings rendered as lowercase
/// hexadecimal, and an all-zero identifier is reserved as "absent". The same
/// three rules are enforced in Rust by <c>validate_hex_id</c>
/// (<c>rust/crates/specgate-runtime/src/capture/snapshot.rs</c>). They are
/// enforced at construction so an invalid identifier cannot reach a trace: a
/// consumer that rejects the artifact gives no clue which producer wrote it.
/// </remarks>
public abstract class CaptureId : IEquatable<CaptureId>
{
    private readonly string _value;

    private protected CaptureId(string value, int length, string label)
    {
        Validate(value, length, label);
        _value = value;
    }

    /// <summary>Gets the lowercase hexadecimal text of this identifier.</summary>
    public string Text => _value;

    /// <summary>Returns the lowercase hexadecimal text of this identifier.</summary>
    /// <returns>The identifier text.</returns>
    public override string ToString() => _value;

    /// <inheritdoc/>
    public bool Equals(CaptureId? other) =>
        other is not null && other.GetType() == GetType() && string.Equals(_value, other._value, StringComparison.Ordinal);

    /// <inheritdoc/>
    public override bool Equals(object? obj) => Equals(obj as CaptureId);

    /// <inheritdoc/>
    public override int GetHashCode() => StringComparer.Ordinal.GetHashCode(_value);

    private static void Validate(string value, int length, string label)
    {
        ArgumentNullException.ThrowIfNull(value);

        if (value.Length != length || !IsLowercaseHex(value) || IsAllZero(value))
        {
            throw new ArgumentException(
                $"specgate: {label} must be a non-zero {length}-character lowercase hexadecimal string, but was '{value}'.",
                nameof(value));
        }
    }

    private static bool IsLowercaseHex(string value)
    {
        foreach (char character in value)
        {
            bool digit = character is >= '0' and <= '9';
            bool lower = character is >= 'a' and <= 'f';
            if (!digit && !lower)
            {
                return false;
            }
        }

        return true;
    }

    private static bool IsAllZero(string value)
    {
        foreach (char character in value)
        {
            if (character != '0')
            {
                return false;
            }
        }

        return true;
    }
}

/// <summary>A validated 32-character trace identifier.</summary>
public sealed class TraceId : CaptureId
{
    /// <summary>The number of hexadecimal characters in a 16-byte trace identifier.</summary>
    public const int Length = 32;

    private TraceId(string value)
        : base(value, Length, "trace ID")
    {
    }

    /// <summary>Validates <paramref name="value"/> and builds a trace identifier.</summary>
    /// <param name="value">Lowercase hexadecimal text.</param>
    /// <returns>The validated trace identifier.</returns>
    /// <exception cref="ArgumentException">The text is the wrong width, is not lowercase hexadecimal, or is all zeroes.</exception>
    public static TraceId Parse(string value) => new(value);
}

/// <summary>A validated 16-character span identifier.</summary>
public sealed class SpanId : CaptureId
{
    /// <summary>The number of hexadecimal characters in an 8-byte span identifier.</summary>
    public const int Length = 16;

    private SpanId(string value)
        : base(value, Length, "span ID")
    {
    }

    /// <summary>Validates <paramref name="value"/> and builds a span identifier.</summary>
    /// <param name="value">Lowercase hexadecimal text.</param>
    /// <returns>The validated span identifier.</returns>
    /// <exception cref="ArgumentException">The text is the wrong width, is not lowercase hexadecimal, or is all zeroes.</exception>
    public static SpanId Parse(string value) => new(value);

    /// <summary>Renders <paramref name="ordinal"/> as a zero-padded span identifier.</summary>
    /// <param name="ordinal">A non-zero sequence number.</param>
    /// <returns>The validated span identifier.</returns>
    /// <exception cref="ArgumentOutOfRangeException"><paramref name="ordinal"/> is zero.</exception>
    internal static SpanId FromOrdinal(ulong ordinal)
    {
        if (ordinal == 0)
        {
            throw new ArgumentOutOfRangeException(nameof(ordinal), "specgate: a span ID ordinal must be non-zero.");
        }

        return new SpanId(ordinal.ToString("x16", System.Globalization.CultureInfo.InvariantCulture));
    }
}
