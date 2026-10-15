using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace SpecGate.Runtime;

/// <summary>
/// Writes a finished <see cref="Capture"/> as native capture sidecar JSON.
/// </summary>
/// <remarks>
/// <para>
/// The sidecar is a protocol, not a convenience format: the CLI reads the same
/// bytes whichever language produced them. This writer therefore reproduces
/// Rust's <c>serde_json</c> output for the same capture - the same field names,
/// the same field order, compact separators, and the externally tagged
/// <c>{"kind":...,"value":...}</c> encoding of a semantic value defined by
/// <c>rust/crates/specgate-runtime/src/value.rs</c>.
/// </para>
/// <para>
/// Fields are written explicitly rather than reflected over, so the wire order
/// is visible here and cannot drift when a property is reordered.
/// </para>
/// </remarks>
public static class CaptureJson
{
    private static readonly JsonWriterOptions WriterOptions = new()
    {
        Indented = false,

        // serde_json escapes only quote, backslash, and control characters. The
        // relaxed encoder matches that; the default encoder would additionally
        // escape HTML-sensitive and non-ASCII characters and diverge from Rust.
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    /// <summary>Serializes <paramref name="capture"/> to sidecar JSON bytes.</summary>
    /// <param name="capture">The finished capture.</param>
    /// <returns>The UTF-8 encoded sidecar document.</returns>
    public static byte[] Serialize(Capture capture)
    {
        if (capture is null)
        {
            throw new ArgumentNullException(nameof(capture));
        }

        using System.IO.MemoryStream stream = new();
        using (Utf8JsonWriter writer = new(stream, WriterOptions))
        {
            WriteCapture(writer, capture);
        }

        return stream.ToArray();
    }

    /// <summary>Serializes <paramref name="capture"/> to a sidecar JSON string.</summary>
    /// <param name="capture">The finished capture.</param>
    /// <returns>The sidecar document.</returns>
    public static string SerializeToString(Capture capture) => Encoding.UTF8.GetString(Serialize(capture));

    private static void WriteCapture(Utf8JsonWriter writer, Capture capture)
    {
        writer.WriteStartObject();
        writer.WriteString("trace_id", capture.TraceId.Text);
        writer.WriteString("scenario_name", capture.ScenarioName);
        writer.WritePropertyName("run");
        WriteBoundary(writer, capture.Run);
        writer.WritePropertyName("scenario");
        WriteBoundary(writer, capture.Scenario);
        writer.WritePropertyName("operations");
        writer.WriteStartArray();
        foreach (OperationSpan operation in capture.Operations)
        {
            WriteOperation(writer, operation);
        }

        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    private static void WriteBoundary(Utf8JsonWriter writer, SpanBoundary boundary)
    {
        writer.WriteStartObject();
        writer.WriteString("span_id", boundary.SpanId.Text);
        if (boundary.ParentSpanId is null)
        {
            writer.WriteNull("parent_span_id");
        }
        else
        {
            writer.WriteString("parent_span_id", boundary.ParentSpanId.Text);
        }

        writer.WriteNumber("start_time_unix_nano", boundary.StartTimeUnixNano);
        writer.WriteNumber("end_time_unix_nano", boundary.EndTimeUnixNano);
        writer.WriteString("status", StatusName(boundary.Status));
        writer.WriteEndObject();
    }

    private static void WriteOperation(Utf8JsonWriter writer, OperationSpan operation)
    {
        writer.WriteStartObject();
        writer.WriteNumber("order", operation.Order);
        writer.WriteString("span_id", operation.SpanId.Text);
        writer.WriteString("parent_span_id", operation.ParentSpanId.Text);
        writer.WriteString("component_id", operation.ComponentId);
        writer.WriteString("operation_name", operation.OperationName);
        writer.WriteNumber("start_time_unix_nano", operation.StartTimeUnixNano);
        writer.WriteNumber("end_time_unix_nano", operation.EndTimeUnixNano);
        writer.WriteString("status", StatusName(operation.Status));

        writer.WritePropertyName("inputs");
        writer.WriteStartObject();
        foreach (KeyValuePair<string, Value> input in operation.Inputs)
        {
            writer.WritePropertyName(input.Key);
            WriteValue(writer, input.Value);
        }

        writer.WriteEndObject();

        writer.WritePropertyName("observations");
        writer.WriteStartArray();
        foreach (Observation observation in operation.Observations)
        {
            writer.WriteStartObject();
            writer.WriteNumber("order", observation.Order);
            writer.WriteNumber("time_unix_nano", observation.TimeUnixNano);
            writer.WriteString("name", observation.Name);
            writer.WritePropertyName("value");
            WriteValue(writer, observation.ObservedValue);
            writer.WriteEndObject();
        }

        writer.WriteEndArray();

        writer.WritePropertyName("completion");
        WriteCompletion(writer, operation.OperationCompletion);
        writer.WriteEndObject();
    }

    private static void WriteCompletion(Utf8JsonWriter writer, Completion? completion)
    {
        if (completion is null)
        {
            writer.WriteNullValue();
            return;
        }

        writer.WriteStartObject();
        writer.WriteString("kind", completion.Kind);
        writer.WriteNumber("order", completion.Order);
        writer.WriteNumber("time_unix_nano", completion.TimeUnixNano);
        switch (completion)
        {
            case ResultCompletion result:
                writer.WritePropertyName("value");
                WriteValue(writer, result.ResultValue);
                break;
            case ErrorCompletion error:
                writer.WriteString("name", error.Name);

                // Rust skips this field entirely when the declared error channel
                // carries no payload, so an absent payload and a null one stay
                // distinguishable on the wire.
                if (error.ErrorValue is not null)
                {
                    writer.WritePropertyName("value");
                    WriteValue(writer, error.ErrorValue);
                }

                break;
            case FaultCompletion fault:
                writer.WriteString("fault_type", fault.FaultType);
                writer.WriteString("message", fault.Message);
                writer.WriteString("observer", fault.Observer);
                break;
            case EmptyCompletion:
            case AbandonedCompletion:
                break;
            default:
                throw new ArgumentException($"specgate: unknown completion channel '{completion.Kind}'.", nameof(completion));
        }

        writer.WriteEndObject();
    }

    private static void WriteValue(Utf8JsonWriter writer, Value value)
    {
        writer.WriteStartObject();
        writer.WriteString("kind", KindName(value.Kind));
        writer.WritePropertyName("value");
        switch (value.Kind)
        {
            case ValueKind.String:
                writer.WriteStringValue(value.AsString());
                break;
            case ValueKind.Integer:
                writer.WriteNumberValue(value.AsInteger());
                break;
            case ValueKind.Unsigned:
                writer.WriteNumberValue(value.AsUnsigned());
                break;
            case ValueKind.Float:
                WriteFloat(writer, value.AsFloat());
                break;
            case ValueKind.Bool:
                writer.WriteBooleanValue(value.AsBool());
                break;
            case ValueKind.List:
                WriteItems(writer, value.AsList());
                break;
            case ValueKind.Set:
                WriteItems(writer, value.AsSet());
                break;
            case ValueKind.Map:
                writer.WriteStartObject();
                foreach (KeyValuePair<string, Value> entry in value.AsMap())
                {
                    writer.WritePropertyName(entry.Key);
                    WriteValue(writer, entry.Value);
                }

                writer.WriteEndObject();
                break;
            default:
                throw new ArgumentException($"specgate: unknown value kind '{value.Kind}'.", nameof(value));
        }

        writer.WriteEndObject();
    }

    private static void WriteItems(Utf8JsonWriter writer, IReadOnlyList<Value> items)
    {
        writer.WriteStartArray();
        foreach (Value item in items)
        {
            WriteValue(writer, item);
        }

        writer.WriteEndArray();
    }

    /// <remarks>
    /// Rust renders a float with the shortest round-trip form and always keeps a
    /// decimal point or an exponent, and <c>serde_json</c> writes a non-finite
    /// float as <c>null</c>. .NET's shortest round-trip form drops the decimal
    /// point and uppercases the exponent, so it is normalized here rather than
    /// leaving the two producers byte-different for the same number.
    /// </remarks>
    private static void WriteFloat(Utf8JsonWriter writer, double value)
    {
        if (double.IsNaN(value) || double.IsInfinity(value))
        {
            writer.WriteNullValue();
            return;
        }

        string text = value.ToString("R", CultureInfo.InvariantCulture);
        int exponent = text.IndexOf('E');
        if (exponent >= 0)
        {
            string mantissa = text.Substring(0, exponent);
            string power = text.Substring(exponent + 1);
            bool negative = power[0] == '-';
            power = power.TrimStart('+', '-').TrimStart('0');
            if (power.Length == 0)
            {
                power = "0";
            }

            text = $"{mantissa}e{(negative ? "-" : string.Empty)}{power}";
        }
        else if (text.IndexOf('.') < 0)
        {
            text += ".0";
        }

        writer.WriteRawValue(text, skipInputValidation: true);
    }

    private static string StatusName(SpanStatus status) => status switch
    {
        SpanStatus.Unset => "unset",
        SpanStatus.Ok => "ok",
        SpanStatus.Error => "error",
        _ => throw new ArgumentOutOfRangeException(nameof(status)),
    };

    private static string KindName(ValueKind kind) => kind switch
    {
        ValueKind.String => "string",
        ValueKind.Integer => "integer",
        ValueKind.Unsigned => "unsigned",
        ValueKind.Float => "float",
        ValueKind.Bool => "bool",
        ValueKind.List => "list",
        ValueKind.Map => "map",
        ValueKind.Set => "set",
        _ => throw new ArgumentOutOfRangeException(nameof(kind)),
    };
}
