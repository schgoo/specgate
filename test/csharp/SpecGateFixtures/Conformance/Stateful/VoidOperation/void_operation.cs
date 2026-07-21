using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Stateful.VoidOperation;

/// <summary>
/// Void operation: <c>log</c> returns nothing but mutates state and echoes an
/// input, verifying capture of inputs and state changes for unit-returning ops.
/// </summary>
public class Logger
{
    /// <summary>The number of messages logged; incremented on each call.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Builds a logger with a zero count.</summary>
    /// <returns>A new <see cref="Logger"/> with <see cref="Count"/> 0.</returns>
    [SpecSetup("log")]
    public static Logger Make() => new() { Count = 0 };

    /// <summary>Records a message, incrementing the count (no return value).</summary>
    /// <param name="msg">The message to log (spec input <c>msg</c>).</param>
    [SpecOperation("log")]
    public void Log([SpecInput("msg")] string msg) => Count += 1;
}
