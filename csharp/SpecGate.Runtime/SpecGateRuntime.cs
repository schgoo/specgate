using System;
using System.ComponentModel;
using System.Threading;

namespace SpecGate.Runtime;

/// <summary>
/// The entry points woven operation boundaries call. Application code does not
/// call these directly.
/// </summary>
/// <remarks>
/// The active operation is published through <see cref="AsyncLocal{T}"/> rather
/// than thread-local storage. <see cref="AsyncLocal{T}"/> flows with the
/// execution context, so an operation remains the parent of work that resumes on
/// a different thread after an <c>await</c>. Thread-local storage would lose the
/// parent at the first continuation that migrates threads.
/// </remarks>
public static class SpecGateRuntime
{
    private static readonly AsyncLocal<OperationScope?> CurrentScope = new();
    private static CaptureSession? _session;

    /// <summary>Gets the operation currently executing on this logical flow.</summary>
    public static OperationScope? Current => CurrentScope.Value;

    /// <summary>Gets whether a capture session is attached.</summary>
    public static bool IsRecording => Volatile.Read(ref _session) is not null;

    /// <summary>Attaches a capture session that records one component.</summary>
    /// <param name="component">The only component recorded by this session.</param>
    /// <exception cref="InvalidOperationException">A session is already active.</exception>
    public static CaptureSession BeginSession(string component)
    {
        if (string.IsNullOrEmpty(component))
        {
            throw new ArgumentException("specgate: a capture session requires a component.", nameof(component));
        }

        CaptureSession session = new(component);
        if (Interlocked.CompareExchange(ref _session, session, null) is not null)
        {
            throw new InvalidOperationException(
                "specgate: a capture session is already active; concurrent sessions are not supported.");
        }

        CurrentScope.Value = null;
        return session;
    }

    internal static void EndSession(CaptureSession session)
    {
        Interlocked.CompareExchange(ref _session, null, session);
        CurrentScope.Value = null;
    }

    /// <summary>Opens an operation declared for a single component. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static OperationScope EnterOperation(
        RuntimeMethodHandle method, string operation, string component)
    {
        CaptureSession? session = Volatile.Read(ref _session);
        if (session is null || !string.Equals(session.Component, component, StringComparison.Ordinal))
        {
            return OperationScope.Disabled;
        }

        return Open(session, operation, component, method);
    }

    /// <summary>
    /// Opens an operation declared for several components. Called by woven code
    /// when a method carries more than one operation annotation.
    /// </summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static OperationScope EnterOperation(
        RuntimeMethodHandle method, string operation, string[] components)
    {
        CaptureSession? session = Volatile.Read(ref _session);
        if (session is null)
        {
            return OperationScope.Disabled;
        }

        if (components is null)
        {
            throw new ArgumentNullException(nameof(components));
        }

        foreach (string component in components)
        {
            if (string.Equals(session.Component, component, StringComparison.Ordinal))
            {
                return Open(session, operation, component, method);
            }
        }

        return OperationScope.Disabled;
    }

    private static OperationScope Open(
        CaptureSession session, string operation, string component, RuntimeMethodHandle method)
    {
        OperationScope scope = new(
            session,
            CurrentScope.Value,
            session.NextSpanId(),
            component,
            operation,
            method);
        CurrentScope.Value = scope;
        return scope;
    }

    /// <summary>Records the operation's inputs. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void RecordInputs(OperationScope scope, object?[] arguments)
    {
        if (scope is null)
        {
            throw new ArgumentNullException(nameof(scope));
        }

        scope.RecordInputs(arguments ?? throw new ArgumentNullException(nameof(arguments)));
    }

    /// <summary>Closes an operation that returned a value. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void ExitOk(OperationScope scope, object? result)
    {
        if (scope is null)
        {
            throw new ArgumentNullException(nameof(scope));
        }

        scope.Complete(
            SpanStatus.Ok,
            scope.IsRecording ? scope.ProjectResult(result) : null,
            exception: null);
        Restore(scope);
    }

    /// <summary>Closes an operation that returned no value. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void ExitVoid(OperationScope scope)
    {
        if (scope is null)
        {
            throw new ArgumentNullException(nameof(scope));
        }

        scope.Complete(SpanStatus.Ok, result: null, exception: null);
        Restore(scope);
    }

    /// <summary>Closes an operation that terminated by throwing. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void ExitError(OperationScope scope, Exception exception)
    {
        if (scope is null)
        {
            throw new ArgumentNullException(nameof(scope));
        }

        scope.Complete(SpanStatus.Error, result: null, exception);
        Restore(scope);
    }

    private static void Restore(OperationScope scope)
    {
        if (scope.IsRecording)
        {
            CurrentScope.Value = scope.Parent;
        }
    }
}
