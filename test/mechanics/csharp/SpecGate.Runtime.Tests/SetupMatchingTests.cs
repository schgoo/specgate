using System.Reflection;
using SpecGate.Annotations;
using SpecGate.TestSupport;
using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Covers how capture selects what each <c>[SpecSetup]</c> contributes to an
/// operation's folded input surface.
/// </summary>
/// <remarks>
/// The declarations below live in this assembly because capture resolves setups
/// by reflecting over the assembly that declares the operation, the way C#
/// discovery does
/// (<c>rust/crates/specgate-discovery/src/csharp_discovery/program.rs:355-365</c>).
/// </remarks>
[Collection(nameof(CaptureSessionSerialization))]
public sealed class SetupMatchingTests
{
    private const string Component = "fixture.setup_matching";

    [Fact]
    public void TwoClrTypesSharingOneEventNameAreOneSemanticType()
    {
        using CaptureSession session = CaptureScenario.Begin(Component);

        // MakeBox returns BoxOne and the operation declares BoxTwo. They are
        // distinct CLR types publishing one registry-visible name, which is what
        // discovery folds on, so the setup still infers the 'box' parameter.
        RunSetup(nameof(SetupDeclarations.MakeBox));
        OperationSpan span = Operate(
            session, nameof(SetupDeclarations.FoldByType), "fold_by_type", [new BoxTwo(), 7]);

        Assert.Equal(["count"], span.Inputs.Keys);
    }

    [Fact]
    public void ASetupMatchingSeveralParametersIsRejected()
    {
        using CaptureSession session = CaptureScenario.Begin(Component);

        RunSetup(nameof(SetupDeclarations.MakeAmbiguous));
        SpecGateCaptureException rejection = Reject(
            session, nameof(SetupDeclarations.Ambiguous), "ambiguous", [null, null]);

        Assert.Equal(CaptureRejection.SetupAmbiguity, rejection.Rejection);
        Assert.Contains("parameters left, right", rejection.Message, StringComparison.Ordinal);
        Assert.Contains("set Fills explicitly", rejection.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ASetupFillingAnUndeclaredParameterIsRejected()
    {
        using CaptureSession session = CaptureScenario.Begin(Component);

        RunSetup(nameof(SetupDeclarations.MakeUnknownFill));
        SpecGateCaptureException rejection = Reject(
            session, nameof(SetupDeclarations.UnknownFill), "unknown_fill", [1]);

        Assert.Equal(CaptureRejection.SetupAmbiguity, rejection.Rejection);
        Assert.Contains("fills unknown parameter 'missing'", rejection.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ANullFilledArgumentIsStillReportedBeforeItIsDiscarded()
    {
        using CaptureSession session = CaptureScenario.Begin(Component);

        RunSetup(nameof(SetupDeclarations.MakeFilled));

        // 'box' is filled by the setup and is not recorded, but an argument that
        // cannot be represented in CTSC is still reported by both producers.
        OperationSpan span = Operate(session, nameof(SetupDeclarations.Filled), "filled", [null, 1]);

        FaultCompletion fault = Assert.IsType<FaultCompletion>(span.OperationCompletion);
        Assert.Equal("specgate.unrecordable_value", fault.FaultType);
        Assert.Contains("parameter 'box'", fault.Message, StringComparison.Ordinal);
        Assert.Empty(span.Inputs);
    }

    [Fact]
    public void ANullSetupInputMakesThePreparedOperationUnrecordable()
    {
        using CaptureSession session = CaptureScenario.Begin(Component);

        // The producer itself still runs and commits; the diagnostic surfaces
        // on the operation it prepares, because that is what cannot be recorded.
        RunSetup(nameof(SetupDeclarations.MakeNamed), [null]);
        OperationSpan span = Operate(
            session, nameof(SetupDeclarations.FromName), "from_name", [new BoxOne(), 2]);

        FaultCompletion fault = Assert.IsType<FaultCompletion>(span.OperationCompletion);
        Assert.Equal("specgate.unrecordable_value", fault.FaultType);
        Assert.Contains("parameter 'name' of setup", fault.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ValueEqualityDistinguishesRepresentations()
    {
        // Rust's values_match is deliberately stricter than semantic equality:
        // a list is not a set and an integer is not a float
        // (rust/crates/specgate-runtime/src/capture/setup.rs:212-237).
        Assert.NotEqual(Value.List([Value.Integer(1)]), Value.Set([Value.Integer(1)]));
        Assert.NotEqual(Value.Integer(1), Value.Float(1.0));
        Assert.Equal(Value.List([Value.Integer(1)]), Value.List([Value.Integer(1)]));
    }

    private static RuntimeMethodHandle Handle(string name) =>
        typeof(SetupDeclarations)
            .GetMethod(name, BindingFlags.Static | BindingFlags.NonPublic)!
            .MethodHandle;

    private static void RunSetup(string name, params object?[] arguments) =>
        SpecGateRuntime.CommitSetup(SpecGateRuntime.StageSetup(Handle(name), arguments));

    private static OperationSpan Operate(
        CaptureSession session, string name, string operation, object?[] arguments)
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(Handle(name), operation, Component);
        SpecGateRuntime.RecordInputs(scope, arguments);
        SpecGateRuntime.ExitOk(scope, 0);

        return Assert.Single(session.Finish().Operations);
    }

    private static SpecGateCaptureException Reject(
        CaptureSession session, string name, string operation, object?[] arguments)
    {
        _ = session;
        OperationScope scope = SpecGateRuntime.EnterOperation(Handle(name), operation, Component);

        return Assert.Throws<SpecGateCaptureException>(
            () => SpecGateRuntime.RecordInputs(scope, arguments));
    }
}

[SpecEvent("Box")]
internal sealed class BoxOne
{
    [SpecEvent("value")]
    public int Value { get; init; }
}

[SpecEvent("Box")]
internal sealed class BoxTwo
{
    [SpecEvent("value")]
    public int Value { get; init; }
}

/// <summary>Declarations standing in for woven setups and the operations they prepare.</summary>
internal static class SetupDeclarations
{
    [SpecSetup("fold_by_type", Spec = "fixture.setup_matching")]
    internal static BoxOne MakeBox() => new();

    [SpecOperation("fold_by_type", Spec = "fixture.setup_matching")]
    internal static int FoldByType([SpecInput("box")] BoxTwo box, [SpecInput("count")] int count) =>
        box.Value + count;

    [SpecSetup("ambiguous", Spec = "fixture.setup_matching")]
    internal static BoxOne MakeAmbiguous() => new();

    [SpecOperation("ambiguous", Spec = "fixture.setup_matching")]
    internal static int Ambiguous([SpecInput("left")] BoxOne left, [SpecInput("right")] BoxTwo right) =>
        left.Value + right.Value;

    [SpecSetup("unknown_fill", Fills = "missing", Spec = "fixture.setup_matching")]
    internal static BoxOne MakeUnknownFill() => new();

    [SpecOperation("unknown_fill", Spec = "fixture.setup_matching")]
    internal static int UnknownFill([SpecInput("count")] int count) => count;

    [SpecSetup("filled", Fills = "box", Spec = "fixture.setup_matching")]
    internal static BoxOne MakeFilled() => new();

    [SpecOperation("filled", Spec = "fixture.setup_matching")]
    internal static int Filled([SpecInput("box")] BoxOne box, [SpecInput("count")] int count) =>
        box.Value + count;

    [SpecSetup("from_name", Spec = "fixture.setup_matching")]
    internal static BoxOne MakeNamed([SpecInput("name")] string name) => new() { Value = name.Length };

    [SpecOperation("from_name", Spec = "fixture.setup_matching")]
    internal static int FromName([SpecInput("box")] BoxOne box, [SpecInput("count")] int count) =>
        box.Value + count;
}
