using System.Globalization;
using System.Text;

namespace SpecGate.Runtime;

/// <summary>
/// A minimal compact JSON writer that reproduces <c>serde_json</c>'s text
/// escaping byte for byte.
/// </summary>
/// <remarks>
/// <c>System.Text.Json</c> cannot be configured to match: it escapes some
/// non-ASCII code points that <c>serde_json</c> writes literally (for example
/// U+2028) and it writes control characters as uppercase <c>\u00XX</c>, where
/// <c>serde_json</c> writes lowercase. <c>serde_json</c> escapes only the quote,
/// the backslash, and characters below U+0020.
/// </remarks>
internal sealed class JsonText
{
    private readonly StringBuilder _text = new();
    private bool _needsComma;
    private bool _afterProperty;

    public override string ToString() => _text.ToString();

    public void WriteStartObject()
    {
        BeforeValue();
        _text.Append('{');
        _needsComma = false;
    }

    public void WriteEndObject()
    {
        _text.Append('}');
        _needsComma = true;
    }

    public void WriteStartArray()
    {
        BeforeValue();
        _text.Append('[');
        _needsComma = false;
    }

    public void WriteEndArray()
    {
        _text.Append(']');
        _needsComma = true;
    }

    public void WritePropertyName(string name)
    {
        if (_needsComma)
        {
            _text.Append(',');
        }

        AppendString(name);
        _text.Append(':');
        _afterProperty = true;
        _needsComma = false;
    }

    public void WriteString(string name, string value)
    {
        WritePropertyName(name);
        WriteStringValue(value);
    }

    public void WriteNull(string name)
    {
        WritePropertyName(name);
        WriteNullValue();
    }

    public void WriteNumber(string name, long value)
    {
        WritePropertyName(name);
        WriteNumberValue(value);
    }

    public void WriteNumber(string name, ulong value)
    {
        WritePropertyName(name);
        WriteNumberValue(value);
    }

    public void WriteStringValue(string value)
    {
        BeforeValue();
        AppendString(value);
        _needsComma = true;
    }

    public void WriteNumberValue(long value) => WriteRawValue(value.ToString(CultureInfo.InvariantCulture));

    public void WriteNumberValue(ulong value) => WriteRawValue(value.ToString(CultureInfo.InvariantCulture));

    public void WriteBooleanValue(bool value) => WriteRawValue(value ? "true" : "false");

    public void WriteNullValue() => WriteRawValue("null");

    public void WriteRawValue(string text)
    {
        BeforeValue();
        _text.Append(text);
        _needsComma = true;
    }

    private void BeforeValue()
    {
        if (!_afterProperty && _needsComma)
        {
            _text.Append(',');
        }

        _afterProperty = false;
    }

    private void AppendString(string value)
    {
        _text.Append('"');
        foreach (char character in value)
        {
            switch (character)
            {
                case '"':
                    _text.Append("\\\"");
                    break;
                case '\\':
                    _text.Append("\\\\");
                    break;
                case '\b':
                    _text.Append("\\b");
                    break;
                case '\f':
                    _text.Append("\\f");
                    break;
                case '\n':
                    _text.Append("\\n");
                    break;
                case '\r':
                    _text.Append("\\r");
                    break;
                case '\t':
                    _text.Append("\\t");
                    break;
                case < ' ':
                    _text.Append("\\u00").Append(((int)character).ToString("x2", CultureInfo.InvariantCulture));
                    break;
                default:
                    _text.Append(character);
                    break;
            }
        }

        _text.Append('"');
    }
}
