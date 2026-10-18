using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

namespace SpecGate.Runtime;

/// <summary>
/// Collects the spans recorded by woven operation boundaries while it is active.
/// </summary>
/// <remarks>
/// <para>
/// A session owns the deterministic protocol state described by its
/// <see cref="CaptureConfig"/>: the logical clock, the event order counter, and
/// the span identifiers handed to operations in start order. It mirrors Rust's
/// <c>State</c> (<c>rust/crates/specgate-runtime/src/capture.rs</c>) so the two
/// producers number and time the same scenario identically.
/// </para>
/// <para>
/// Disposing the session detaches it, so operations that run afterwards record
/// nothing. A session is process-wide; concurrent sessions are rejected rather
/// than silently interleaved.
/// </para>
/// </remarks>
public sealed class CaptureSession : IDisposable
{
    private readonly List<PendingOperation> _operations = [];
    private readonly Dictionary<SetupKey, SortedDictionary<string, Value>> _setups = [];
    private readonly object _gate = new();
    private readonly object _persistGate = new();
    private readonly SidecarWriter? _sidecar;
    private readonly long _runStartNs;
    private readonly long _scenarioStartNs;
    private long _nextTimeNs;
    private ulong _nextOrder;
    private int _nextOp;
    private bool _detached;
    private bool _finished;

    /// <param name="component">
    /// The only component recorded, or <see langword="null"/> to record every
    /// component as the Rust runtime does when activated from the environment.
    /// </param>
    /// <param name="config">The deterministic protocol configuration.</param>
    /// <param name="sidecar">Where to rewrite the capture after each operation, if anywhere.</param>
    internal CaptureSession(string? component, CaptureConfig config, SidecarWriter? sidecar)
    {
        Component = component ?? string.Empty;
        RecordsAllComponents = component is null;
        Config = config;
        _sidecar = sidecar;
        _runStartNs = config.StartTimeUnixNano;
        _scenarioStartNs = Add(config.StartTimeUnixNano, config.ClockStepUnixNano);
        _nextTimeNs = Add(_scenarioStartNs, config.ClockStepUnixNano);
    }

    /// <summary>Gets the component this session records, or an empty string when it records every component.</summary>
    public string Component { get; }

    /// <summary>Gets whether this session records operations of every component.</summary>
    internal bool RecordsAllComponents { get; }

    /// <summary>Gets whether operations of <paramref name="component"/> are recorded.</summary>
    internal bool Records(string component) =>
        RecordsAllComponents || string.Equals(Component, component, StringComparison.Ordinal);

    /// <summary>Gets the deterministic protocol configuration.</summary>
    public CaptureConfig Config { get; }

    /// <summary>Finishes the session and builds the capture.</summary>
    /// <returns>The finished capture.</returns>
    /// <exception cref="InvalidOperationException">
    /// The session was already finished, an operation scope is still open, or
    /// the configuration supplied operation span identifiers that were never
    /// used.
    /// </exception>
    public Capture Finish()
    {
        lock (_gate)
        {
            if (_finished)
            {
                throw new InvalidOperationException("specgate: this capture session was already finished.");
            }

            _finished = true;
            Detach();

            // An operation that never reached a terminal channel is recorded as
            // a fault on its own span before the failure is reported, so the
            // evidence names the boundary that was still open rather than only
            // reporting that the capture was incomplete.
            List<PendingOperation> outstanding = [.. _operations.Where(operation => operation.Status is null)];
            if (outstanding.Count > 0)
            {
                string names = string.Join(" -> ", outstanding.Select(operation => operation.OperationName));
                CloseOutstanding(outstanding);
                throw new InvalidOperationException(
                    $"specgate: capture has nested/unclosed operation scopes: {names}");
            }

            if (_nextOp < Config.OperationSpanIds.Count)
            {
                throw new InvalidOperationException(
                    $"specgate: capture supplied {Config.OperationSpanIds.Count} operation span IDs "
                    + $"but consumed {_nextOp}.");
            }

            return Build();
        }
    }

    private Capture Build() =>
        BuildCapture([.. _operations.Select(operation => operation.Build())], _nextTimeNs);

    private Capture BuildCapture(OperationSpan[] operations, long scenarioEndNs)
    {
        long runEndNs = Add(scenarioEndNs, Config.ClockStepUnixNano);
        bool hasError = operations.Any(operation => operation.Status == SpanStatus.Error);
        SpanStatus rootStatus = hasError ? SpanStatus.Error : SpanStatus.Ok;

        return new Capture(
            Config.TraceId,
            Config.ScenarioName,
            new SpanBoundary(Config.RunSpanId, null, _runStartNs, runEndNs, rootStatus),
            new SpanBoundary(Config.ScenarioSpanId, Config.RunSpanId, _scenarioStartNs, scenarioEndNs, rootStatus),
            operations);
    }

    /// <summary>
    /// Projects the capture as it stands without ending the session. An
    /// operation that has not reached a terminal channel is shown as an
    /// <c>incomplete_capture</c> fault, the same evidence <see cref="Finish"/>
    /// records, so a reader can tell a finished capture from an interrupted one.
    /// </summary>
    /// <remarks>
    /// The clock and order counters are copied, so projecting never shifts the
    /// timestamps of operations that run afterwards.
    /// </remarks>
    private Capture Snapshot()
    {
        lock (_gate)
        {
            long nextTimeNs = _nextTimeNs;
            ulong nextOrder = _nextOrder;
            Dictionary<PendingOperation, OperationSpan> projected = [];
            for (int index = _operations.Count - 1; index >= 0; index--)
            {
                PendingOperation operation = _operations[index];
                if (operation.Status is not null)
                {
                    continue;
                }

                ulong order = nextOrder++;
                long faultNs = nextTimeNs;
                nextTimeNs = Add(nextTimeNs, Config.ClockStepUnixNano);
                long endNs = nextTimeNs;
                nextTimeNs = Add(nextTimeNs, Config.ClockStepUnixNano);
                projected.Add(operation, operation.BuildAs(IncompleteFault(operation, order, faultNs), endNs));
            }

            OperationSpan[] spans =
            [
                .. _operations.Select(operation =>
                    projected.TryGetValue(operation, out OperationSpan? span) ? span : operation.Build()),
            ];
            return BuildCapture(spans, nextTimeNs);
        }
    }

    private static FaultCompletion IncompleteFault(PendingOperation operation, ulong order, long timeNs) => new(
        order,
        timeNs,
        "incomplete_capture",
        $"native operation '{operation.OperationName}' was still outstanding when the capture ended",
        NativeFault.TargetObserver);

    /// <summary>
    /// Rewrites the sidecar from the current state when this session has one.
    /// </summary>
    /// <exception cref="SpecGateCaptureException">The sidecar could not be replaced.</exception>
    internal void Persist()
    {
        if (_sidecar is null)
        {
            return;
        }

        // Serialized so two operations finishing together cannot replace a newer
        // snapshot with an older one.
        lock (_persistGate)
        {
            _sidecar.Write(CaptureJson.Serialize(Snapshot()));
        }
    }

    /// <summary>
    /// Final flush at process exit. Never throws: the process is ending and the
    /// previous sidecar already shows every outstanding operation as incomplete.
    /// </summary>
    internal void PersistAtExit()
    {
        bool outstanding;
        lock (_gate)
        {
            outstanding = _operations.Any(operation => operation.Status is null);
        }

        if (!outstanding)
        {
            return;
        }

        try
        {
            Persist();
        }
        catch (SpecGateCaptureException)
        {
            // The earlier persisted snapshot stands; a failure here has nobody to report to.
        }
    }

    private void CloseOutstanding(List<PendingOperation> outstanding)
    {
        for (int index = outstanding.Count - 1; index >= 0; index--)
        {
            PendingOperation operation = outstanding[index];
            ulong order = Order();
            operation.Complete(IncompleteFault(operation, order, Tick()), Tick());
        }
    }

    /// <summary>Registers a starting operation and returns its mutable record.</summary>
    /// <remarks>
    /// The clock is read in the same order as Rust's <c>begin_active</c>: the
    /// span identifier is allocated, then the start timestamp, then the start
    /// order. Reordering these would shift every later timestamp.
    /// </remarks>
    internal PendingOperation Begin(string component, string operation, SpanId? parentSpanId)
    {
        lock (_gate)
        {
            if (_finished)
            {
                throw new InvalidOperationException(
                    "specgate: cannot begin an operation after the capture session has finished.");
            }

            SpanId spanId = NextSpanId();
            long startNs = Tick();
            ulong order = Order();
            PendingOperation pending = new(
                order,
                spanId,
                parentSpanId ?? Config.ScenarioSpanId,
                component,
                operation,
                startNs);
            _operations.Add(pending);
            _nextOp++;
            return pending;
        }
    }

    /// <summary>Allocates the clock and order values one completion consumes.</summary>
    /// <remarks>
    /// A unit operation passes <see langword="null"/>: it reports no completion
    /// event, so it consumes an end timestamp and nothing else, exactly as Rust's
    /// <c>NativeTerminal::Unit</c> does.
    /// </remarks>
    internal void Complete(PendingOperation operation, Func<ulong, long, Completion>? completion)
    {
        lock (_gate)
        {
            if (operation.Status is not null)
            {
                throw new InvalidOperationException(
                    $"specgate: operation '{operation.OperationName}' cannot be completed twice.");
            }

            Completion? built = null;
            if (completion is not null)
            {
                ulong order = Order();
                built = completion(order, Tick());
            }

            operation.Complete(built, Tick());
        }

        Persist();
    }

    /// <remarks>
    /// A supplied identifier is handed out in start order. Otherwise the session
    /// numbers operations itself, skipping any synthesized value that would
    /// collide with a boundary span or an already-allocated operation.
    /// </remarks>
    private SpanId NextSpanId()
    {
        if (_nextOp < Config.OperationSpanIds.Count)
        {
            return Config.OperationSpanIds[_nextOp];
        }

        ulong candidate = checked((ulong)_nextOp + 1);
        while (true)
        {
            string text = candidate.ToString("x16", CultureInfo.InvariantCulture);
            bool reserved = string.Equals(text, Config.RunSpanId.Text, StringComparison.Ordinal)
                || string.Equals(text, Config.ScenarioSpanId.Text, StringComparison.Ordinal)
                || _operations.Any(operation => string.Equals(operation.SpanId.Text, text, StringComparison.Ordinal));
            if (!reserved)
            {
                return SpanId.FromOrdinal(candidate);
            }

            candidate = checked(candidate + 1);
        }
    }

    private long Tick()
    {
        long current = _nextTimeNs;
        _nextTimeNs = Add(current, Config.ClockStepUnixNano);
        return current;
    }

    private ulong Order()
    {
        ulong current = _nextOrder;
        _nextOrder = checked(current + 1);
        return current;
    }

    private static long Add(long left, long right)
    {
        try
        {
            return checked(left + right);
        }
        catch (OverflowException error)
        {
            throw new InvalidOperationException("specgate: capture logical clock overflow.", error);
        }
    }

    /// <summary>
    /// Records the construction inputs of one committed setup declaration.
    /// </summary>
    /// <remarks>
    /// <para>
    /// This is the C# home of Rust's <c>PENDING</c> store
    /// (<c>rust/crates/specgate-runtime/src/capture/setup.rs:5-9</c>). Rust
    /// keys it to a thread; C# keys it to the session, because an
    /// <c>AsyncLocal</c> written inside a setup producer would not flow back to
    /// the caller that later invokes the operation, and thread-affine state is
    /// exactly what the runtime is moving away from. Session lifetime is the same
    /// boundary as Rust's <c>clear()</c>.
    /// </para>
    /// <para>
    /// Running one declaration twice is accepted only when both runs record
    /// value-identical inputs, mirroring <c>record_values</c>
    /// (<c>capture/setup.rs:151-202</c>).
    /// </para>
    /// </remarks>
    internal void RecordSetup(SetupKey key, SortedDictionary<string, Value> inputs)
    {
        lock (_gate)
        {
            if (!_setups.TryGetValue(key, out SortedDictionary<string, Value>? earlier))
            {
                _setups.Add(key, inputs);
                return;
            }

            if (!Matches(earlier, inputs))
            {
                throw new SpecGateCaptureException(
                    CaptureRejection.SetupAmbiguity,
                    $"specgate: setup '{key.Producer}' for '{key.Component}::{key.Operation}' ran twice "
                    + $"in one capture with different inputs ({Describe(earlier)} then {Describe(inputs)}); "
                    + "capture attributes construction inputs by declaration and cannot tell which instance "
                    + $"a later '{key.Operation}' invocation used. Capture one construction per scenario, "
                    + "or record identical inputs.");
            }
        }
    }

    /// <summary>
    /// The construction inputs committed for one setup declaration, or
    /// <see langword="null"/> when it has not run successfully in this session.
    /// </summary>
    internal SortedDictionary<string, Value>? SetupInputs(SetupKey key)
    {
        lock (_gate)
        {
            return _setups.TryGetValue(key, out SortedDictionary<string, Value>? inputs) ? inputs : null;
        }
    }

    /// <remarks>
    /// Rust's <c>values_match</c> (<c>capture/setup.rs:212-237</c>) is
    /// deliberately stricter than value equality: a list never matches a set and
    /// an integer never matches a float. <see cref="Value"/> compares by
    /// <see cref="ValueKind"/> first and floats by bit pattern, so its own
    /// equality is already exactly that; adding a second comparison here would
    /// only create a way for the two to drift apart.
    /// </remarks>
    private static bool Matches(
        SortedDictionary<string, Value> earlier, SortedDictionary<string, Value> later)
    {
        if (earlier.Count != later.Count)
        {
            return false;
        }

        foreach (KeyValuePair<string, Value> entry in earlier)
        {
            if (!later.TryGetValue(entry.Key, out Value? recorded) || !entry.Value.Equals(recorded))
            {
                return false;
            }
        }

        return true;
    }

    private static string Describe(SortedDictionary<string, Value> inputs) =>
        string.Join(", ", inputs.Select(entry => $"{entry.Key}={entry.Value}"));

    /// <summary>Detaches this session so later operations are no longer recorded.</summary>
    public void Dispose()
    {
        lock (_gate)
        {
            Detach();
        }
    }

    private void Detach()
    {
        if (_detached)
        {
            return;
        }

        _detached = true;
        SpecGateRuntime.EndSession(this);
    }
}

/// <summary>Stable fault vocabulary shared by both capture producers.</summary>
internal static class NativeFault
{
    /// <summary>The fault type recorded when annotated target code throws an unmapped exception.</summary>
    internal const string UnexpectedTargetFault = "specgate.unexpected_target_fault";

    /// <summary>The fault message recorded when annotated target code throws an unmapped exception.</summary>
    internal const string UnwoundMessage = "operation unwound before returning";

    /// <summary>The stable observer identifier for target-reported faults.</summary>
    internal const string TargetObserver = "target";
}

/// <summary>One operation span while it is still being recorded.</summary>
internal sealed class PendingOperation
{
    private IReadOnlyDictionary<string, Value> _inputs = Capture.NoInputs;
    private Completion? _completion;
    private long _endTimeUnixNano;

    internal PendingOperation(
        ulong order,
        SpanId spanId,
        SpanId parentSpanId,
        string componentId,
        string operationName,
        long startTimeUnixNano)
    {
        Order = order;
        SpanId = spanId;
        ParentSpanId = parentSpanId;
        ComponentId = componentId;
        OperationName = operationName;
        StartTimeUnixNano = startTimeUnixNano;
    }

    internal ulong Order { get; }

    internal SpanId SpanId { get; }

    internal SpanId ParentSpanId { get; }

    internal string ComponentId { get; }

    internal string OperationName { get; }

    internal long StartTimeUnixNano { get; }

    internal SpanStatus? Status { get; private set; }

    internal void RecordInputs(IReadOnlyDictionary<string, Value> inputs) => _inputs = inputs;

    internal void Complete(Completion? completion, long endTimeUnixNano)
    {
        _completion = completion;
        _endTimeUnixNano = endTimeUnixNano;
        Status = completion?.RequiredStatus ?? SpanStatus.Ok;
    }

    internal OperationSpan Build() => BuildAs(
        _completion,
        _endTimeUnixNano,
        Status ?? throw new InvalidOperationException(
            $"specgate: operation '{OperationName}' has no terminal status."));

    /// <summary>Builds the span as though it had ended with <paramref name="completion"/>, leaving this record open.</summary>
    internal OperationSpan BuildAs(Completion completion, long endTimeUnixNano) =>
        BuildAs(completion, endTimeUnixNano, completion.RequiredStatus);

    private OperationSpan BuildAs(Completion? completion, long endTimeUnixNano, SpanStatus status) => new(
        Order,
        SpanId,
        ParentSpanId,
        ComponentId,
        OperationName,
        StartTimeUnixNano,
        endTimeUnixNano,
        status,
        _inputs,
        Capture.NoObservations,
        completion);
}
