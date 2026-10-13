using System;
using System.Collections.Generic;

namespace SpecGate.Runtime;

/// <summary>
/// Orders strings by their UTF-8 byte sequence, which is the order Rust's
/// <c>BTreeMap&lt;String, _&gt;</c> and <c>String</c> comparisons use.
/// </summary>
/// <remarks>
/// UTF-8 byte order equals Unicode code point order, so this compares code
/// points rather than UTF-16 code units. The two disagree for supplementary
/// characters: <c>U+10000</c> encodes as the surrogate pair <c>D800 DC00</c>,
/// which <see cref="StringComparer.Ordinal"/> sorts below <c>U+E000</c> even
/// though its code point is far above it. Using ordinal order here would make
/// C# and Rust captures of the same map disagree on key order.
/// </remarks>
internal sealed class Utf8KeyComparer : IComparer<string>
{
    internal static Utf8KeyComparer Instance { get; } = new();

    public int Compare(string? x, string? y)
    {
        if (ReferenceEquals(x, y))
        {
            return 0;
        }

        if (x is null)
        {
            return -1;
        }

        if (y is null)
        {
            return 1;
        }

        int left = 0;
        int right = 0;
        while (left < x.Length && right < y.Length)
        {
            int leftPoint = NextCodePoint(x, ref left);
            int rightPoint = NextCodePoint(y, ref right);
            if (leftPoint != rightPoint)
            {
                return leftPoint < rightPoint ? -1 : 1;
            }
        }

        return (x.Length - left).CompareTo(y.Length - right);
    }

    /// <summary>
    /// Reads the code point starting at <paramref name="index"/> and advances it.
    /// An unpaired surrogate is returned as its own code unit; capture rejects
    /// such strings before they reach this comparer.
    /// </summary>
    private static int NextCodePoint(string text, ref int index)
    {
        char current = text[index];
        if (char.IsHighSurrogate(current)
            && index + 1 < text.Length
            && char.IsLowSurrogate(text[index + 1]))
        {
            index += 2;
            return char.ConvertToUtf32(current, text[index - 1]);
        }

        index++;
        return current;
    }
}
