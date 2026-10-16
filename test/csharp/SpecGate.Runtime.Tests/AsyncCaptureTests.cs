using System.Runtime.CompilerServices;
using SpecGate.TestSupport;
using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Drives the observers woven async operations call in place of the synchronous
/// exits. Each test plays the compiler's kick-off stub: it opens the operation,
/// starts the body, and hands the resulting awaitable to the runtime.
/// </summary>
[Collection(nameof(CaptureSessionSerialization))]
public sealed class AsyncCaptureTests
{
    [Fact]
    public async Task ATaskOfTRecordsItsAwaitedValueNotTheTask()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        string result = await StartAsync("FetchAsync", "fetch", async () =>
        {
            await Task.Yield();
            return "response";
        });

        Assert.Equal("response", result);
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(Value.String("response"), span.Result());
    }

    [Fact]
    public async Task ANonGenericTaskRecordsNoCompletion()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        await StartAsync("ResetAsync", "reset", async () => await Task.Yield());

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Null(span.OperationCompletion);
    }

    [Fact]
    public async Task RecordingBeginsWhenTheOperationStartsNotWhenItIsAwaited()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");
        TaskCompletionSource<string> gate = new(TaskCreationOptions.RunContinuationsAsynchronously);

        Task<string> pending = Observe("FetchAsync", "fetch", async () => await gate.Task);
        Assert.Null(SpecGateRuntime.Current);

        gate.SetResult("late");
        Assert.Equal("late", await pending);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(Value.String("late"), span.Result());
    }

    [Fact]
    public async Task TheCallerNextOperationIsNotNestedUnderAnAsyncOperationItStarted()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");
        TaskCompletionSource<string> gate = new(TaskCreationOptions.RunContinuationsAsynchronously);

        Task<string> first = Observe("FetchAsync", "first", async () => await gate.Task);
        Task<string> second = Observe("FetchAsync", "second", () => Task.FromResult("done"));
        gate.SetResult("ok");
        await Task.WhenAll(first, second);

        Capture capture = session.Finish();
        Assert.Equal(CaptureScenario.ScenarioSpanId, capture.Operation("first").ParentSpanId);
        Assert.Equal(CaptureScenario.ScenarioSpanId, capture.Operation("second").ParentSpanId);
    }

    [Fact]
    public async Task AnOperationStartedAfterAThreadHopNestsUnderItsAsyncCaller()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        await StartAsync("FetchAsync", "outer", async () =>
        {
            int enteredOn = Environment.CurrentManagedThreadId;
            await DedicatedThreadHop.Instance;
            Assert.NotEqual(enteredOn, Environment.CurrentManagedThreadId);

            await StartAsync("FetchAsync", "inner", () => Task.FromResult("inner"));
            return "outer";
        });

        Capture capture = session.Finish();
        Assert.Equal(capture.Operation("outer").SpanId, capture.Operation("inner").ParentSpanId);
    }

    [Fact]
    public async Task InterleavedAsyncOperationsKeepTheirOwnParents()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");
        TaskCompletionSource<bool> release = new(TaskCreationOptions.RunContinuationsAsynchronously);

        Task<string> Run(string name) => Observe("FetchAsync", name, async () =>
        {
            await release.Task;
            await StartAsync("FetchAsync", name + ".child", () => Task.FromResult(name));
            return name;
        });

        Task<string> left = Run("left");
        Task<string> right = Run("right");
        release.SetResult(true);
        await Task.WhenAll(left, right);

        Capture capture = session.Finish();
        Assert.Equal(capture.Operation("left").SpanId, capture.Operation("left.child").ParentSpanId);
        Assert.Equal(capture.Operation("right").SpanId, capture.Operation("right.child").ParentSpanId);
        Assert.Equal(CaptureScenario.ScenarioSpanId, capture.Operation("left").ParentSpanId);
        Assert.Equal(CaptureScenario.ScenarioSpanId, capture.Operation("right").ParentSpanId);
    }

    [Fact]
    public async Task AThrownExceptionIsRecordedAndRethrown()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        InvalidOperationException thrown = await Assert.ThrowsAsync<InvalidOperationException>(
            () => StartAsync<string>("FetchAsync", "fetch", async () =>
            {
                await Task.Yield();
                throw new InvalidOperationException("boom");
            }));

        Assert.Equal("boom", thrown.Message);
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Error, span.Status);
        Assert.IsType<FaultCompletion>(span.OperationCompletion);
    }

    [Fact]
    public async Task ACanceledTaskIsRecordedAsAbandoned()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");
        using CancellationTokenSource cancellation = new();

        Task<string> pending = Observe("FetchAsync", "fetch", async () =>
        {
            await Task.Delay(Timeout.Infinite, cancellation.Token);
            return "unreachable";
        });
        cancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => pending);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Unset, span.Status);
        Assert.IsType<AbandonedCompletion>(span.OperationCompletion);
    }

    [Fact]
    public async Task AnAbandonedChildDoesNotFailItsParent()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");
        using CancellationTokenSource cancellation = new();
        cancellation.Cancel();

        await StartAsync("FetchAsync", "parent", async () =>
        {
            try
            {
                await StartAsync(
                    "FetchAsync", "child", () => Task.FromCanceled<string>(cancellation.Token));
            }
            catch (OperationCanceledException)
            {
            }

            return "recovered";
        });

        Capture capture = session.Finish();
        Assert.Equal(SpanStatus.Unset, capture.Operation("child").Status);
        Assert.Equal(SpanStatus.Ok, capture.Operation("parent").Status);
    }

    [Fact]
    public async Task AnOperationStillRunningAtFinishFailsClosed()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");
        TaskCompletionSource<string> never = new(TaskCreationOptions.RunContinuationsAsynchronously);

        _ = Observe("FetchAsync", "fetch", async () => await never.Task);

        Assert.Throws<InvalidOperationException>(() => session.Finish());
        await Task.CompletedTask;
    }

    [Fact]
    public async Task AValueTaskOfTRecordsItsAwaitedValue()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        string sync = await StartValue("FetchValueAsync", "sync", () => new ValueTask<string>("cached"));
        string async = await StartValueFrom("FetchValueAsync", "async", async () =>
        {
            await Task.Yield();
            return "fetched";
        });

        Assert.Equal("cached", sync);
        Assert.Equal("fetched", async);
        Capture capture = session.Finish();
        Assert.Equal(Value.String("cached"), capture.Operation("sync").Result());
        Assert.Equal(Value.String("fetched"), capture.Operation("async").Result());
    }

    [Fact]
    public async Task ANonGenericValueTaskRecordsNoCompletion()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        await StartValue("ResetValueAsync", "sync", () => default(ValueTask));
        await StartValueFrom("ResetValueAsync", "async", async () => await Task.Yield());

        Capture capture = session.Finish();
        Assert.Null(capture.Operation("sync").OperationCompletion);
        Assert.Equal(SpanStatus.Ok, capture.Operation("async").Status);
    }

    [Fact]
    public async Task AFailedValueTaskIsRecordedAndRethrown()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        await Assert.ThrowsAsync<InvalidOperationException>(async () =>
            await StartValueFrom<string>("FetchValueAsync", "fetch", async () =>
            {
                await Task.Yield();
                throw new InvalidOperationException("boom");
            }));

        Assert.Equal(SpanStatus.Error, Assert.Single(session.Finish().Operations).Status);
    }

    private static Task<T> StartAsync<T>(string method, string operation, Func<Task<T>> body) =>
        Observe(method, operation, body);

    private static Task StartAsync(string method, string operation, Func<Task> body)
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of(method), operation, "fixture.async");
        SpecGateRuntime.RecordInputs(scope, []);
        return SpecGateRuntime.ObserveTask(scope, body());
    }

    private static Task<T> Observe<T>(string method, string operation, Func<Task<T>> body)
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of(method), operation, "fixture.async");
        SpecGateRuntime.RecordInputs(scope, []);
        return SpecGateRuntime.ObserveTask(scope, body());
    }

    private static ValueTask<T> StartValueFrom<T>(string method, string operation, Func<Task<T>> body) =>
        StartValue(method, operation, () => new ValueTask<T>(body()));

    private static ValueTask<T> StartValue<T>(string method, string operation, Func<ValueTask<T>> body)
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of(method), operation, "fixture.async");
        SpecGateRuntime.RecordInputs(scope, []);
        return SpecGateRuntime.ObserveValueTask(scope, body());
    }

    private static ValueTask StartValue(string method, string operation, Func<ValueTask> body)
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of(method), operation, "fixture.async");
        SpecGateRuntime.RecordInputs(scope, []);
        return SpecGateRuntime.ObserveValueTask(scope, body());
    }

    private static ValueTask StartValueFrom(string method, string operation, Func<Task> body) =>
        StartValue(method, operation, () => new ValueTask(body()));

    /// <summary>
    /// Awaitable that always resumes on a brand-new dedicated thread so the
    /// resumed managed thread id is guaranteed to differ from the awaiting one.
    /// </summary>
    private sealed class DedicatedThreadHop : INotifyCompletion
    {
        public static DedicatedThreadHop Instance { get; } = new();

        public bool IsCompleted => false;

        public DedicatedThreadHop GetAwaiter() => this;

        public void OnCompleted(Action continuation)
        {
            Thread worker = new(() => continuation()) { IsBackground = true };
            worker.Start();
        }

        public void GetResult()
        {
        }
    }
}
