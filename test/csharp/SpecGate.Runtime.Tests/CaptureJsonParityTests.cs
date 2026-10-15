using System.Text;
using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Asserts C# writes the same native capture sidecar bytes as Rust.
/// </summary>
/// <remarks>
/// <para>
/// The CLI reads one sidecar format regardless of which language produced it,
/// so the two producers must agree on field order, field naming, optional-field
/// omission, and value encoding. That agreement is written down once, as bytes,
/// in <c>test/goldens/capture-sidecar/parity.json</c>.
/// </para>
/// <para>
/// Rust owns those bytes: <c>sidecar_parity.rs</c> in <c>specgate-runtime</c>
/// builds this same capture and fails first when the protocol changes. This
/// test builds it from the public C# model and confirms the bytes match. The
/// file is a hand-maintained cross-language contract, not a generated CTSC
/// golden, which is why it does not live under <c>test/goldens/ctsc</c>.
/// </para>
/// </remarks>
public class CaptureJsonParityTests
{
    private const string Component = "parity.component";

    [Fact]
    public void TheSidecarDocumentMatchesTheCrossLanguageGolden()
    {
        string golden = File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "parity.json")).TrimEnd('\n');

        string actual = CaptureJson.SerializeToString(ParityCapture());

        Assert.Equal(golden, actual);
    }

    [Fact]
    public void TheSidecarIsWrittenAsUtf8WithoutAByteOrderMark()
    {
        byte[] bytes = CaptureJson.Serialize(ParityCapture());

        Assert.Equal(Encoding.UTF8.GetBytes(CaptureJson.SerializeToString(ParityCapture())), bytes);
        Assert.Equal((byte)'{', bytes[0]);
    }

    /// <summary>
    /// A capture that reaches every completion channel, every value kind, and
    /// both optional-field states, so the golden pins the whole wire surface.
    /// </summary>
    private static Capture ParityCapture() => new(
        TraceId.Parse("0af7651916cd43dd8448eb211c80319c"),
        "parity",
        new SpanBoundary(SpanId.Parse("aaaaaaaaaaaa0001"), null, 0, 100, SpanStatus.Error),
        new SpanBoundary(
            SpanId.Parse("aaaaaaaaaaaa0002"), SpanId.Parse("aaaaaaaaaaaa0001"), 1, 99, SpanStatus.Error),
        [
            EveryValueKind(),
            DeclaredErrorWithPayload(),
            DeclaredErrorWithoutPayload(),
            UnmappedFault(),
            ExplicitEmpty(),
            Abandoned(),
            UnitWithoutCompletion(),
            TextAndNumbers(),
        ]);

    /// <summary>
    /// Strings and floats whose serde_json spelling is easy to get wrong, with
    /// inputs supplied out of order and keys whose UTF-8 order differs from
    /// their UTF-16 order, so the constructor must sort them.
    /// </summary>
    private static OperationSpan TextAndNumbers()
    {
        double[] floats = [1e-5, 1e-6, 1e15, 1e16, 1.5e300, 5e-324, -0.0, -2.5, 123456789.125, 0.1];
        Dictionary<string, Value> inputs = new()
        {
            ["text"] = Value.String("\u2028\U0001F600\u001f\b\f\t\u007f\u00e9/"),
            ["\U0001F600"] = Value.Bool(false),
            ["floats"] = Value.List([.. floats.Select(Value.Float)]),
            ["\uffff"] = Value.Bool(true),
        };

        return new OperationSpan(
            15,
            SpanId.Parse("aaaaaaaaaaaa0017"),
            SpanId.Parse("aaaaaaaaaaaa0002"),
            Component,
            "text_and_numbers",
            28,
            30,
            SpanStatus.Ok,
            inputs,
            [],
            new EmptyCompletion(16, 29));
    }

    /// <summary>
    /// A result completion whose inputs reach every semantic value kind, plus
    /// one observation, so the golden pins the value encoding exhaustively.
    /// </summary>
    private static OperationSpan EveryValueKind()
    {
        SortedDictionary<string, Value> inputs = new(StringComparer.Ordinal)
        {
            ["a_string"] = Value.String("hello \"world\"\n"),
            ["b_integer"] = Value.Integer(long.MinValue),
            ["c_unsigned"] = Value.Unsigned(ulong.MaxValue),
            ["d_float"] = Value.Float(1.5),
            ["e_bool"] = Value.Bool(true),
            ["f_list"] = Value.List([Value.Integer(1), Value.Integer(2)]),
            ["g_map"] = Value.Map(
            [
                new KeyValuePair<string, Value>("inner", Value.Bool(false)),
                new KeyValuePair<string, Value>("outer", Value.String("x")),
            ]),
            ["h_set"] = Value.Set([Value.String("a"), Value.String("b")]),
            ["i_unit"] = Value.Unit,
        };

        return new OperationSpan(
            0,
            SpanId.Parse("aaaaaaaaaaaa0010"),
            SpanId.Parse("aaaaaaaaaaaa0002"),
            Component,
            "every_value_kind",
            2,
            5,
            SpanStatus.Ok,
            inputs,
            [new Observation(1, 3, "progress", Value.Unsigned(1))],
            new ResultCompletion(2, 4, Value.Integer(7)));
    }

    /// <summary>A declared error that carries a payload.</summary>
    private static OperationSpan DeclaredErrorWithPayload() => Span(
        3,
        "aaaaaaaaaaaa0011",
        "aaaaaaaaaaaa0010",
        "error_with_payload",
        6,
        9,
        SpanStatus.Error,
        new ErrorCompletion(4, 8, "error", Value.String("boom")));

    /// <summary>
    /// A declared error whose value channel is empty, pinning the one optional
    /// field the protocol omits rather than writes as null.
    /// </summary>
    private static OperationSpan DeclaredErrorWithoutPayload() => Span(
        5,
        "aaaaaaaaaaaa0012",
        "aaaaaaaaaaaa0002",
        "error_without_payload",
        10,
        13,
        SpanStatus.Error,
        new ErrorCompletion(6, 12, "error", null));

    /// <summary>An unmapped target failure.</summary>
    private static OperationSpan UnmappedFault() => Span(
        7,
        "aaaaaaaaaaaa0013",
        "aaaaaaaaaaaa0002",
        "unmapped_fault",
        14,
        17,
        SpanStatus.Error,
        new FaultCompletion(8, 16, "specgate.unexpected_target_fault", "operation unwound before returning", "target"));

    /// <summary>A successful absence from an operation that has a value channel.</summary>
    private static OperationSpan ExplicitEmpty() => Span(
        9, "aaaaaaaaaaaa0014", "aaaaaaaaaaaa0002", "explicit_empty", 18, 21, SpanStatus.Ok, new EmptyCompletion(10, 20));

    /// <summary>An operation that stopped without reaching any outcome.</summary>
    private static OperationSpan Abandoned() => Span(
        11,
        "aaaaaaaaaaaa0015",
        "aaaaaaaaaaaa0002",
        "abandoned",
        22,
        25,
        SpanStatus.Unset,
        new AbandonedCompletion(12, 24));

    /// <summary>A unit operation, which has no value channel and so no completion.</summary>
    private static OperationSpan UnitWithoutCompletion() => Span(
        13, "aaaaaaaaaaaa0016", "aaaaaaaaaaaa0002", "unit", 26, 27, SpanStatus.Ok, null);

    private static OperationSpan Span(
        ulong order,
        string spanId,
        string parentSpanId,
        string operationName,
        long startTimeUnixNano,
        long endTimeUnixNano,
        SpanStatus status,
        Completion? completion) =>
        new(
            order,
            SpanId.Parse(spanId),
            SpanId.Parse(parentSpanId),
            Component,
            operationName,
            startTimeUnixNano,
            endTimeUnixNano,
            status,
            new SortedDictionary<string, Value>(StringComparer.Ordinal),
            [],
            completion);
}
