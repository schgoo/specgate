using SpecGate.Annotations;

namespace SpecGate.WeaverFixtures;

/// <summary>
/// Async fixtures whose compiler-generated stubs the weaver rewrites. Each shape
/// the weaver supports (<c>Task</c>, <c>Task&lt;T&gt;</c>, <c>ValueTask</c>,
/// <c>ValueTask&lt;T&gt;</c>) appears once, with a body that really suspends so the
/// span must outlive the synchronous portion of the call.
/// </summary>
public static class AsyncOperations
{
    [SpecOperation("echo_i32", Spec = "weaver.generic", TypeArguments = new[] { typeof(int) })]
    [SpecOperation("echo_string", Spec = "weaver.generic", TypeArguments = new[] { typeof(string) })]
    public static async Task<T> EchoAsync<T>([SpecInput("value")] T value)
    {
        await Task.Yield();
        return value;
    }

    [SpecEvent]
    public sealed class Item
    {
        [SpecEvent]
        public int Value { get; set; } = 7;
    }

    [SpecOperation("items", Spec = "weaver.generic", TypeArguments = new[] { typeof(Item) })]
    public static async Task<List<T>> ItemsAsync<T>() where T : notnull, new()
    {
        await Task.Yield();
        return [new T()];
    }

    [SpecOperation("double", Spec = "weaver.async")]
    public static async Task<int> DoubleAsync([SpecInput("value")] int value)
    {
        await Task.Yield();
        return value * 2;
    }

    [SpecOperation("touch", Spec = "weaver.async")]
    public static async Task TouchAsync([SpecInput("value")] int value)
    {
        await Task.Yield();
        _ = value;
    }

    [SpecOperation("double_value", Spec = "weaver.async")]
    public static async ValueTask<int> DoubleValueAsync([SpecInput("value")] int value)
    {
        await Task.Yield();
        return value * 2;
    }

    [SpecOperation("touch_value", Spec = "weaver.async")]
    public static async ValueTask TouchValueAsync([SpecInput("value")] int value)
    {
        await Task.Yield();
        _ = value;
    }

    [SpecOperation("fail", Spec = "weaver.async")]
    public static async Task<int> FailAsync([SpecInput("value")] int value)
    {
        await Task.Yield();
        throw new InvalidOperationException($"failed {value}");
    }

    /// <summary>Cancelled by tests to end <see cref="WaitAsync"/>; a token parameter has no CTSC representation.</summary>
    public static CancellationTokenSource Gate { get; set; } = new();

    [SpecOperation("wait", Spec = "weaver.async")]
    public static async Task<int> WaitAsync([SpecInput("value")] int value)
    {
        await Task.Delay(Timeout.Infinite, Gate.Token);
        return value;
    }

    /// <summary>Completed by tests to settle <see cref="Forward"/>.</summary>
    public static TaskCompletionSource<int> Pending { get; set; } = new();

    [SpecOperation("forward", Spec = "weaver.async")]
    public static Task<int> Forward([SpecInput("value")] int value) => Pending.Task;

    [SpecOperation("forward_unit", Spec = "weaver.async")]
    public static Task ForwardUnit([SpecInput("value")] int value) => Pending.Task;

    [SpecOperation("outer", Spec = "weaver.async")]
    public static async Task<int> OuterAsync([SpecInput("value")] int value)
    {
        int doubled = await DoubleAsync(value);
        await Task.Yield();
        return doubled + 1;
    }
}
