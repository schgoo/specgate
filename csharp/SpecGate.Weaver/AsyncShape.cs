namespace SpecGate.Weaver;

/// <summary>The awaitable an annotated method returns.</summary>
internal enum AsyncShape
{
    /// <summary>The method is not async.</summary>
    None,

    /// <summary>An async method returning <c>Task</c>.</summary>
    Task,

    /// <summary>An async method returning <c>Task&lt;T&gt;</c>.</summary>
    TaskOfT,

    /// <summary>An async method returning <c>ValueTask</c>.</summary>
    ValueTask,

    /// <summary>An async method returning <c>ValueTask&lt;T&gt;</c>.</summary>
    ValueTaskOfT,

    /// <summary>An async method returning anything else, including <c>void</c>.</summary>
    Unsupported,
}
