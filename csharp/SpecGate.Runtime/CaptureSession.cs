using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;

namespace SpecGate.Runtime;

/// <summary>
/// Collects the spans recorded by woven operation boundaries while it is active.
/// </summary>
/// <remarks>
/// Disposing the session detaches it, so operations that run afterwards record
/// nothing. A session is process-wide; concurrent sessions are rejected rather
/// than silently interleaved.
/// </remarks>
public sealed class CaptureSession : IDisposable
{
    private readonly List<RecordedSpan> _spans = [];
    private readonly Dictionary<SetupKey, SortedDictionary<string, Value>> _setups = [];
    private readonly object _gate = new();
    private long _nextSpanId;
    private bool _disposed;

    internal CaptureSession(string component) => Component = component;

    /// <summary>Gets the component this session records.</summary>
    public string Component { get; }

    /// <summary>Gets the spans recorded so far, in completion order.</summary>
    public IReadOnlyList<RecordedSpan> Spans
    {
        get
        {
            lock (_gate)
            {
                return [.. _spans];
            }
        }
    }

    internal long NextSpanId() => Interlocked.Increment(ref _nextSpanId);

    internal void Record(RecordedSpan span)
    {
        lock (_gate)
        {
            _spans.Add(span);
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
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        SpecGateRuntime.EndSession(this);
    }
}
