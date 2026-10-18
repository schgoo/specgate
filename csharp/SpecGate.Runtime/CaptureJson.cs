using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;

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
    /// <summary>Serializes <paramref name="capture"/> to sidecar JSON bytes.</summary>
    /// <param name="capture">The finished capture.</param>
    /// <returns>The UTF-8 encoded sidecar document.</returns>
    public static byte[] Serialize(Capture capture)
    {
        ArgumentNullException.ThrowIfNull(capture);

        JsonText writer = new();
        WriteCapture(writer, capture);
        return new UTF8Encoding(encoderShouldEmitUTF8Identifier: false).GetBytes(writer.ToString());
    }

    /// <summary>Serializes <paramref name="capture"/> to a sidecar JSON string.</summary>
    /// <param name="capture">The finished capture.</param>
    /// <returns>The sidecar document.</returns>
    public static string SerializeToString(Capture capture) => Encoding.UTF8.GetString(Serialize(capture));

    private static void WriteCapture(JsonText writer, Capture capture)
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

    private static void WriteBoundary(JsonText writer, SpanBoundary boundary)
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

    private static void WriteOperation(JsonText writer, OperationSpan operation)
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

    private static void WriteCompletion(JsonText writer, Completion? completion)
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

    private static void WriteValue(JsonText writer, Value value)
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

    private static void WriteItems(JsonText writer, IReadOnlyList<Value> items)
    {
        writer.WriteStartArray();
        foreach (Value item in items)
        {
            WriteValue(writer, item);
        }

        writer.WriteEndArray();
    }

    /// <remarks>
    /// Reproduces the float text <c>serde_json</c> writes: the shortest
    /// round-trip digits, positional notation while the decimal point falls
    /// within <c>1e-5</c> up to below <c>1e16</c>, scientific notation outside
    /// that range (<c>1e-6</c>, <c>1e+16</c>), always a decimal point in
    /// positional form, and <c>null</c> for a non-finite float. .NET's shortest
    /// round-trip text supplies the digits; the notation is chosen here so both
    /// producers agree on the bytes.
    /// </remarks>
    private static void WriteFloat(JsonText writer, double value)
    {
        if (double.IsNaN(value) || double.IsInfinity(value))
        {
            writer.WriteNullValue();
            return;
        }

        writer.WriteRawValue(FormatFloat(value));
    }

    private static string FormatFloat(double value)
    {
        bool negative = value < 0 || (value == 0 && BitConverter.DoubleToInt64Bits(value) < 0);
        if (value == 0)
        {
            return negative ? "-0.0" : "0.0";
        }

        string text = Math.Abs(value).ToString("R", CultureInfo.InvariantCulture);
        int exponentAt = text.IndexOf('E');
        int exponent = exponentAt >= 0 ? int.Parse(text.AsSpan(exponentAt + 1), provider: CultureInfo.InvariantCulture) : 0;
        string mantissa = exponentAt >= 0 ? text.Substring(0, exponentAt) : text;
        int pointAt = mantissa.IndexOf('.');
        int integerLength = pointAt >= 0 ? pointAt : mantissa.Length;
        string digits = mantissa.Replace(".", string.Empty);

        // The value is the digit string read as an integer times 10^scale.
        int scale = exponent + integerLength - digits.Length;
        digits = digits.TrimStart('0');
        string significant = digits.TrimEnd('0');
        scale += digits.Length - significant.Length;
        int length = significant.Length;
        int point = length + scale;

        string body;
        if (scale >= 0 && point <= 16)
        {
            body = significant + new string('0', scale) + ".0";
        }
        else if (point > 0 && point <= 16)
        {
            body = string.Concat(significant.AsSpan(0, point), ".", significant.AsSpan(point));
        }
        else if (point > -5 && point <= 0)
        {
            body = "0." + new string('0', -point) + significant;
        }
        else
        {
            int scientificExponent = point - 1;
            string sign = scientificExponent < 0 ? "-" : "+";
            string scientific = Math.Abs(scientificExponent).ToString(CultureInfo.InvariantCulture);
            body = length == 1
                ? significant + "e" + sign + scientific
                : significant.Substring(0, 1) + "." + significant.Substring(1) + "e" + sign + scientific;
        }

        return negative ? "-" + body : body;
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
