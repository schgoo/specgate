using SpecGate.Runtime;

namespace SpecGate.TestSupport;

/// <summary>
/// Starts capture sessions with one canonical deterministic configuration.
/// </summary>
/// <remarks>
/// A capture is deterministic because its driver supplies the identifiers and
/// the logical clock, so every test would otherwise have to invent the same
/// four identifiers. Sharing them here also means a test that asserts on exact
/// span identifiers or timestamps is asserting on one documented configuration
/// rather than on a local literal.
/// </remarks>
public static class CaptureScenario
{
    /// <summary>The trace identifier every scenario built here uses.</summary>
    public const string TraceIdText = "0af7651916cd43dd8448eb211c80319c";

    /// <summary>The run span identifier every scenario built here uses.</summary>
    public const string RunSpanIdText = "aaaaaaaaaaaa0001";

    /// <summary>The scenario span identifier every scenario built here uses.</summary>
    public const string ScenarioSpanIdText = "aaaaaaaaaaaa0002";

    /// <summary>The default scenario name.</summary>
    public const string DefaultScenarioName = "scenario";

    /// <summary>Builds the canonical configuration, optionally with supplied operation span IDs.</summary>
    public static CaptureConfig Config(
        string scenarioName = DefaultScenarioName, params string[] operationSpanIds)
    {
        CaptureConfigBuilder builder = CaptureConfig.Builder(
            scenarioName,
            TraceId.Parse(TraceIdText),
            SpanId.Parse(RunSpanIdText),
            SpanId.Parse(ScenarioSpanIdText));

        if (operationSpanIds.Length > 0)
        {
            builder.OperationSpanIds(operationSpanIds.Select(SpanId.Parse));
        }

        return builder.Build();
    }

    /// <summary>Starts a session recording one component with the canonical configuration.</summary>
    public static CaptureSession Begin(string component, string scenarioName = DefaultScenarioName) =>
        SpecGateRuntime.BeginSession(component, Config(scenarioName));

    /// <summary>The scenario span identifier, which is the parent of every root operation.</summary>
    public static SpanId ScenarioSpanId => SpanId.Parse(ScenarioSpanIdText);

    /// <summary>The single span recorded for <paramref name="operation"/>.</summary>
    public static OperationSpan Operation(this Capture capture, string operation)
    {
        ArgumentNullException.ThrowIfNull(capture);
        return capture.Operations.Single(span =>
            string.Equals(span.OperationName, operation, StringComparison.Ordinal));
    }

    /// <summary>The result value recorded for <paramref name="span"/>.</summary>
    public static Value? Result(this OperationSpan span)
    {
        ArgumentNullException.ThrowIfNull(span);
        return (span.OperationCompletion as ResultCompletion)?.ResultValue;
    }

    /// <summary>The recorded inputs in semantic name order.</summary>
    public static IReadOnlyList<KeyValuePair<string, Value>> OrderedInputs(this OperationSpan span)
    {
        ArgumentNullException.ThrowIfNull(span);
        return [.. span.Inputs];
    }
}
