using System;
using System.ComponentModel;
using System.Runtime.ExceptionServices;
using System.Threading;
using System.Threading.Tasks;

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
    /// <param name="config">The deterministic protocol configuration.</param>
    /// <exception cref="InvalidOperationException">A session is already active.</exception>
    public static CaptureSession BeginSession(string component, CaptureConfig config)
    {
        if (string.IsNullOrEmpty(component))
        {
            throw new ArgumentException("specgate: a capture session requires a component.", nameof(component));
        }

        ArgumentNullException.ThrowIfNull(config);

        CaptureSession session = new(component, config, sidecar: null);
        return Attach(session);
    }

    /// <summary>
    /// Attaches a session driven by the native-capture environment protocol.
    /// It records every component, as the Rust runtime does, and rewrites its
    /// sidecar after each completed operation.
    /// </summary>
    /// <returns>The attached session, or <see langword="null"/> when a session is already active.</returns>
    internal static CaptureSession? BeginEnvironmentSession(CaptureConfig config, SidecarWriter sidecar)
    {
        CaptureSession session = new(component: null, config, sidecar);
        if (Interlocked.CompareExchange(ref _session, session, null) is not null)
        {
            return null;
        }

        CurrentScope.Value = null;
        return session;
    }

    private static CaptureSession Attach(CaptureSession session)
    {
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
        if (session is null || !session.Records(component))
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

        ArgumentNullException.ThrowIfNull(components);

        foreach (string component in components)
        {
            if (session.Records(component))
            {
                return Open(session, operation, component, method);
            }
        }

        return OperationScope.Disabled;
    }

    private static OperationScope Open(
        CaptureSession session, string operation, string component, RuntimeMethodHandle method)
    {
        OperationScope? parent = CurrentScope.Value;
        PendingOperation pending = session.Begin(component, operation, parent?.Pending?.SpanId);
        OperationScope scope = new(session, parent, pending, component, operation, method);
        CurrentScope.Value = scope;
        return scope;
    }

    /// <summary>Opens a setup producer and projects its construction inputs. Called by woven code.</summary>
    /// <remarks>
    /// Returns <see langword="null"/> when nothing is recording or the method
    /// declares no setup for the session's component.
    /// </remarks>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static SetupStaging? StageSetup(RuntimeMethodHandle method, object?[] arguments)
    {
        CaptureSession? session = Volatile.Read(ref _session);
        if (session is null)
        {
            return null;
        }

        return SetupStaging.Stage(
            session, method, arguments ?? throw new ArgumentNullException(nameof(arguments)));
    }

    /// <summary>Commits a setup producer's provenance. Called by woven code.</summary>
    /// <remarks>
    /// The woven call sits on the producer's success path only, so a setup that
    /// throws commits nothing and the operation it prepares then fails with a
    /// missing-provenance rejection.
    /// </remarks>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void CommitSetup(SetupStaging? staged) => staged?.Commit();

    /// <summary>Records the operation's inputs. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void RecordInputs(OperationScope scope, object?[] arguments)
    {
        ArgumentNullException.ThrowIfNull(scope);

        try
        {
            scope.RecordInputs(arguments ?? throw new ArgumentNullException(nameof(arguments)));
        }
        catch
        {
            // The woven try region begins after this call, so no epilogue closes
            // this scope. Leaving it current would parent the caller's next
            // operation to an operation that never completed.
            Restore(scope);
            throw;
        }
    }

    /// <summary>Closes an operation that returned a value. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void ExitOk(OperationScope scope, object? result)
    {
        ArgumentNullException.ThrowIfNull(scope);

        try
        {
            scope.CompleteValue(scope.IsRecording ? scope.ProjectResult(result) : null);
        }
        finally
        {
            // The woven epilogue runs outside the try region, so a rejected
            // result would otherwise leave this scope current.
            Restore(scope);
        }
    }

    /// <summary>Closes an operation that returned no value. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void ExitVoid(OperationScope scope)
    {
        ArgumentNullException.ThrowIfNull(scope);

        try
        {
            scope.CompleteValue(null);
        }
        finally
        {
            Restore(scope);
        }
    }

    /// <summary>Closes an operation that terminated by throwing. Called by woven code.</summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static void ExitError(OperationScope scope, Exception exception)
    {
        ArgumentNullException.ThrowIfNull(scope);

        try
        {
            scope.CompleteThrown(exception ?? throw new ArgumentNullException(nameof(exception)));
        }
        finally
        {
            Restore(scope);
        }
    }

    /// <summary>
    /// Completes an asynchronous operation when its task settles. Called by woven
    /// code in place of <see cref="ExitOk"/>.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The woven method is the compiler's non-async kick-off stub, so by the time
    /// it reaches here the body has run to its first suspension point and the
    /// operation is already open: recording begins at the call, never at some
    /// later construction step. The scope is restored for the caller on return,
    /// while the state machine's captured execution context keeps the scope
    /// current for every continuation.
    /// </para>
    /// <para>
    /// The returned task replaces the original. It settles after the operation
    /// has been completed, so the caller's next operation cannot be recorded
    /// before this one ends.
    /// </para>
    /// </remarks>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static Task ObserveTask(OperationScope scope, Task task)
    {
        RequireObservable(scope, task);
        try
        {
            return scope.IsRecording ? ObserveTaskCore(scope, task) : task;
        }
        finally
        {
            Restore(scope);
        }
    }

    /// <inheritdoc cref="ObserveTask(OperationScope, Task)"/>
    /// <typeparam name="T">The awaited result type.</typeparam>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static Task<T> ObserveTask<T>(OperationScope scope, Task<T> task)
    {
        RequireObservable(scope, task);
        try
        {
            return scope.IsRecording ? ObserveTaskCore(scope, task) : task;
        }
        finally
        {
            Restore(scope);
        }
    }

    /// <inheritdoc cref="ObserveTask(OperationScope, Task)"/>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static ValueTask ObserveValueTask(OperationScope scope, ValueTask task)
    {
        RequireScope(scope);
        try
        {
            return scope.IsRecording ? ObserveValueTaskCore(scope, task) : task;
        }
        finally
        {
            Restore(scope);
        }
    }

    /// <inheritdoc cref="ObserveTask(OperationScope, Task)"/>
    /// <typeparam name="T">The awaited result type.</typeparam>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static ValueTask<T> ObserveValueTask<T>(OperationScope scope, ValueTask<T> task)
    {
        RequireScope(scope);
        try
        {
            return scope.IsRecording ? ObserveValueTaskCore(scope, task) : task;
        }
        finally
        {
            Restore(scope);
        }
    }

    private static async Task ObserveTaskCore(OperationScope scope, Task task)
    {
        Exception? failure = null;
        try
        {
            await task.ConfigureAwait(false);
        }
        catch (Exception exception)
        {
            failure = exception;
        }

        if (failure is null)
        {
            scope.CompleteValue(null);
            return;
        }

        FinishFailed(scope, failure, task.IsCanceled);
    }

    private static async Task<T> ObserveTaskCore<T>(OperationScope scope, Task<T> task)
    {
        Exception? failure = null;
        T result = default!;
        try
        {
            result = await task.ConfigureAwait(false);
        }
        catch (Exception exception)
        {
            failure = exception;
        }

        if (failure is null)
        {
            scope.CompleteValue(scope.ProjectResult(result));
            return result;
        }

        FinishFailed(scope, failure, task.IsCanceled);
        return result;
    }

    private static async ValueTask ObserveValueTaskCore(OperationScope scope, ValueTask task)
    {
        Exception? failure = null;
        bool canceled = false;
        try
        {
            await task.ConfigureAwait(false);
        }
        catch (Exception exception)
        {
            failure = exception;
            canceled = exception is OperationCanceledException;
        }

        if (failure is null)
        {
            scope.CompleteValue(null);
            return;
        }

        FinishFailed(scope, failure, canceled);
    }

    private static async ValueTask<T> ObserveValueTaskCore<T>(OperationScope scope, ValueTask<T> task)
    {
        Exception? failure = null;
        bool canceled = false;
        T result = default!;
        try
        {
            result = await task.ConfigureAwait(false);
        }
        catch (Exception exception)
        {
            failure = exception;
            canceled = exception is OperationCanceledException;
        }

        if (failure is null)
        {
            scope.CompleteValue(scope.ProjectResult(result));
            return result;
        }

        FinishFailed(scope, failure, canceled);
        return result;
    }

    /// <summary>
    /// Completes the span for a task that did not return, then rethrows the
    /// original exception with its stack intact.
    /// </summary>
    /// <remarks>
    /// A canceled task is the operation being discarded by its caller before it
    /// reached an outcome, which is what Rust records as abandonment when a
    /// future is dropped. Any other exception is a thrown operation.
    /// </remarks>
    private static void FinishFailed(OperationScope scope, Exception failure, bool canceled)
    {
        if (canceled)
        {
            scope.CompleteAbandoned();
        }
        else
        {
            scope.CompleteThrown(failure);
        }

        ExceptionDispatchInfo.Capture(failure).Throw();
    }

    private static void RequireObservable(OperationScope scope, Task task)
    {
        RequireScope(scope);
        ArgumentNullException.ThrowIfNull(task);
    }

    private static void RequireScope(OperationScope scope) => ArgumentNullException.ThrowIfNull(scope);

    private static void Restore(OperationScope scope)
    {
        if (scope.IsRecording)
        {
            CurrentScope.Value = scope.Parent;
        }
    }
}
