namespace SpecGate.Runtime;

/// <summary>The terminal channel an operation's returned value is recorded on.</summary>
internal enum ReturnChannel
{
    /// <summary>The <c>result</c> channel, carrying the returned value.</summary>
    Result,

    /// <summary>The <c>empty</c> channel: an optional return that was absent.</summary>
    Empty,

    /// <summary>The <c>error</c> channel: a result return that failed.</summary>
    Error,
}

/// <summary>A projected return value together with the channel it belongs on.</summary>
internal readonly struct ReturnOutcome
{
    internal ReturnOutcome(ReturnChannel channel, Value? value)
    {
        Channel = channel;
        Value = value;
    }

    internal ReturnChannel Channel { get; }

    /// <summary>Gets the projected value, or <see langword="null"/> on the empty channel.</summary>
    internal Value? Value { get; }
}
