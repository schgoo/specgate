using System;
using System.Collections.Generic;
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
