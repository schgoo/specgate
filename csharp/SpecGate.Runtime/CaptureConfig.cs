using System;
using System.Collections.Generic;

namespace SpecGate.Runtime;

/// <summary>
/// The deterministic protocol configuration for one capture session.
/// </summary>
/// <remarks>
/// <para>
/// Nothing about a capture is read from the machine: trace and span identities
/// are supplied, and timestamps come from a logical clock that starts at
/// <see cref="StartTimeUnixNano"/> and advances by
/// <see cref="ClockStepUnixNano"/>. Two runs of the same scenario therefore
/// produce byte-identical evidence, which is the only reason a later capture
/// can be compared against an earlier one.
/// </para>
/// <para>
/// This mirrors Rust's <c>Config</c>
/// (<c>rust/crates/specgate-runtime/src/capture/model.rs</c>) field for field,
/// including its wire names, so both producers write the same sidecar shape.
/// </para>
/// </remarks>
public sealed class CaptureConfig
{
    /// <summary>The logical clock origin used when none is supplied.</summary>
    public const long DefaultStartTimeUnixNano = 0;

    /// <summary>The logical clock increment used when none is supplied.</summary>
    public const long DefaultClockStepUnixNano = 1;

    internal CaptureConfig(
        string scenarioName,
        TraceId traceId,
        SpanId runSpanId,
        SpanId scenarioSpanId,
        IReadOnlyList<SpanId> operationSpanIds,
        long startTimeUnixNano,
        long clockStepUnixNano)
    {
        ScenarioName = scenarioName;
        TraceId = traceId;
        RunSpanId = runSpanId;
        ScenarioSpanId = scenarioSpanId;
        OperationSpanIds = operationSpanIds;
        StartTimeUnixNano = startTimeUnixNano;
        ClockStepUnixNano = clockStepUnixNano;
    }

    /// <summary>Gets the stable scenario name.</summary>
    public string ScenarioName { get; }

    /// <summary>Gets the trace identifier shared by every span in this capture.</summary>
    public TraceId TraceId { get; }

    /// <summary>Gets the run span identifier.</summary>
    public SpanId RunSpanId { get; }

    /// <summary>Gets the scenario span identifier.</summary>
    public SpanId ScenarioSpanId { get; }

    /// <summary>
    /// Gets the operation span identifiers handed out in start order.
    /// </summary>
    /// <remarks>
    /// An empty list lets the session number operations itself. A supplied list
    /// must be consumed exactly: a capture that uses fewer identifiers than it
    /// was given ran fewer operations than the caller expected.
    /// </remarks>
    public IReadOnlyList<SpanId> OperationSpanIds { get; }

    /// <summary>Gets the logical clock origin in Unix nanoseconds.</summary>
    public long StartTimeUnixNano { get; }

    /// <summary>Gets the logical clock increment in Unix nanoseconds.</summary>
    public long ClockStepUnixNano { get; }

    /// <summary>Starts a builder from the required protocol identities.</summary>
    /// <param name="scenarioName">The stable scenario name.</param>
    /// <param name="traceId">The capture trace identifier.</param>
    /// <param name="runSpanId">The run span identifier.</param>
    /// <param name="scenarioSpanId">The scenario span identifier.</param>
    /// <returns>A builder that validates on <see cref="CaptureConfigBuilder.Build"/>.</returns>
    public static CaptureConfigBuilder Builder(
        string scenarioName, TraceId traceId, SpanId runSpanId, SpanId scenarioSpanId) =>
        new(scenarioName, traceId, runSpanId, scenarioSpanId);
}

/// <summary>Builds a validated <see cref="CaptureConfig"/>.</summary>
public sealed class CaptureConfigBuilder
{
    private readonly string _scenarioName;
    private readonly TraceId _traceId;
    private readonly SpanId _runSpanId;
    private readonly SpanId _scenarioSpanId;
    private SpanId[] _operationSpanIds = Array.Empty<SpanId>();
    private long _startTimeUnixNano = CaptureConfig.DefaultStartTimeUnixNano;
    private long _clockStepUnixNano = CaptureConfig.DefaultClockStepUnixNano;

    internal CaptureConfigBuilder(string scenarioName, TraceId traceId, SpanId runSpanId, SpanId scenarioSpanId)
    {
        _scenarioName = scenarioName ?? throw new ArgumentNullException(nameof(scenarioName));
        _traceId = traceId ?? throw new ArgumentNullException(nameof(traceId));
        _runSpanId = runSpanId ?? throw new ArgumentNullException(nameof(runSpanId));
        _scenarioSpanId = scenarioSpanId ?? throw new ArgumentNullException(nameof(scenarioSpanId));
    }

    /// <summary>Supplies the operation span identifiers to hand out in start order.</summary>
    /// <param name="ids">The identifiers.</param>
    /// <returns>This builder.</returns>
    public CaptureConfigBuilder OperationSpanIds(IEnumerable<SpanId> ids)
    {
        ArgumentNullException.ThrowIfNull(ids);

        _operationSpanIds = [.. ids];
        return this;
    }

    /// <summary>Sets the logical clock origin.</summary>
    /// <param name="value">A non-negative Unix nanosecond timestamp.</param>
    /// <returns>This builder.</returns>
    public CaptureConfigBuilder StartTime(long value)
    {
        _startTimeUnixNano = value;
        return this;
    }

    /// <summary>Sets the logical clock increment.</summary>
    /// <param name="value">A positive nanosecond step.</param>
    /// <returns>This builder.</returns>
    public CaptureConfigBuilder ClockStep(long value)
    {
        _clockStepUnixNano = value;
        return this;
    }

    /// <summary>Validates and builds the configuration.</summary>
    /// <returns>The validated configuration.</returns>
    /// <exception cref="ArgumentException">A clock value is out of range or a span identifier is reused.</exception>
    public CaptureConfig Build()
    {
        if (_startTimeUnixNano < 0)
        {
            throw new ArgumentException("specgate: a capture start timestamp must be non-negative.");
        }

        if (_clockStepUnixNano <= 0)
        {
            throw new ArgumentException("specgate: a capture logical clock step must be positive.");
        }

        HashSet<string> spanIds = new(StringComparer.Ordinal) { _runSpanId.Text };
        if (!spanIds.Add(_scenarioSpanId.Text))
        {
            throw new ArgumentException("specgate: capture span IDs must be unique.");
        }

        for (int index = 0; index < _operationSpanIds.Length; index++)
        {
            SpanId candidate = _operationSpanIds[index]
                ?? throw new ArgumentException($"specgate: operation span ID at index {index} is null.");
            if (!spanIds.Add(candidate.Text))
            {
                throw new ArgumentException($"specgate: capture operation span ID at index {index} is duplicated.");
            }
        }

        return new CaptureConfig(
            _scenarioName,
            _traceId,
            _runSpanId,
            _scenarioSpanId,
            _operationSpanIds,
            _startTimeUnixNano,
            _clockStepUnixNano);
    }
}
