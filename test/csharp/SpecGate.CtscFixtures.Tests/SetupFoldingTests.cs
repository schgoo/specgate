using System.Reflection;
using SpecGate.Runtime;
using Xunit;
using MultiField = SpecGate.CtscFixtures.Conformance.Stateful.MultiFieldCapture;
using MultiSetup = SpecGate.CtscFixtures.Conformance.Stateful.MultiSetup;
using SetupWithParams = SpecGate.CtscFixtures.Conformance.Stateful.SetupWithParams;
using Shared = SpecGate.CtscFixtures.Conformance.Stateful.SharedSetup;

namespace SpecGate.CtscFixtures.Tests;

/// <summary>
/// Covers the folded input surface C# capture records for the conformance
/// fixtures that declare <c>[SpecSetup]</c>.
/// </summary>
/// <remarks>
/// These fixtures are compiled unwoven, so the capture entry points are driven
/// directly with the fixture methods' own handles. That exercises the real
/// assembly scan and the real folding — the woven IL shape is covered separately
/// by the weaver's own fixture.
/// </remarks>
public sealed class SetupFoldingTests
{
    [Fact]
    public void AReceiverSetupContributesNoRecordedInput()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.multi_field_capture");

        RunSetup(typeof(MultiField.Account), nameof(MultiField.Account.Make));
        RecordedSpan span = Operate(
            session, typeof(MultiField.Account), nameof(MultiField.Account.Withdraw), "withdraw", [25]);

        Assert.Equal(["amount"], span.Inputs.Select(input => input.Name));
        Assert.Equal(Value.Integer(25), span.Inputs[0].Value);
    }

    [Fact]
    public void ExplicitFillsReplaceTheParametersTheyName()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.multi_setup");

        MultiSetup.Account source = MultiSetup.Transfer.MakeSource();
        MultiSetup.Account target = MultiSetup.Transfer.MakeTarget();
        RunSetup(typeof(MultiSetup.Transfer), nameof(MultiSetup.Transfer.MakeSource));
        RunSetup(typeof(MultiSetup.Transfer), nameof(MultiSetup.Transfer.MakeTarget));

        RecordedSpan span = Operate(
            session,
            typeof(MultiSetup.Transfer),
            nameof(MultiSetup.Transfer.Execute),
            "transfer",
            [source, target, 30]);

        Assert.Equal(["amount"], span.Inputs.Select(input => input.Name));
    }

    [Fact]
    public void OneProducerSatisfiesEveryDeclarationItCarries()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.shared_setup");

        Shared.BoxVal box = Shared.Combine.MakeBox();
        RunSetup(typeof(Shared.Combine), nameof(Shared.Combine.MakeBox));

        RecordedSpan span = Operate(
            session, typeof(Shared.Combine), nameof(Shared.Combine.Two), "combine", [box, box], 4);

        Assert.Empty(span.Inputs);
    }

    [Fact]
    public void ASetupsOwnInputsBecomeInputsOfTheOperation()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.setup_with_params");

        RunSetup(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make), 5);
        RecordedSpan span = Operate(
            session,
            typeof(SetupWithParams.Counter),
            nameof(SetupWithParams.Counter.Increment),
            "increment",
            []);

        Assert.Equal(["initial"], span.Inputs.Select(input => input.Name));
        Assert.Equal(Value.Integer(5), span.Inputs[0].Value);
    }

    [Fact]
    public void AnOperationWhoseSetupNeverRanIsRejected()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.multi_field_capture");

        SpecGateCaptureException rejection = Reject(
            session, typeof(MultiField.Account), nameof(MultiField.Account.Withdraw), "withdraw", [25]);

        Assert.Equal(CaptureRejection.SetupMissing, rejection.Rejection);
        Assert.Contains("constructing the receiver", rejection.Message, StringComparison.Ordinal);

        // A rejection raised while folding abandons its own scope, so nothing is
        // left current for the caller's next operation to be parented to.
        Assert.Null(SpecGateRuntime.Current);
        Assert.Empty(session.Spans);
    }

    [Fact]
    public void ASetupThatDoesNotReturnSuccessfullyCommitsNothing()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.setup_with_params");

        // A throwing producer stages its inputs and never reaches the woven
        // CommitSetup call, which is exactly this sequence.
        _ = SpecGateRuntime.StageSetup(
            Handle(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make)), [5]);

        SpecGateCaptureException rejection = Reject(
            session,
            typeof(SetupWithParams.Counter),
            nameof(SetupWithParams.Counter.Increment),
            "increment",
            []);

        Assert.Equal(CaptureRejection.SetupMissing, rejection.Rejection);
    }

    [Fact]
    public void ASetupMayRunTwiceWithIdenticalInputs()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.setup_with_params");

        RunSetup(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make), 5);
        RunSetup(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make), 5);

        RecordedSpan span = Operate(
            session,
            typeof(SetupWithParams.Counter),
            nameof(SetupWithParams.Counter.Increment),
            "increment",
            []);

        Assert.Equal(Value.Integer(5), Assert.Single(span.Inputs).Value);
    }

    [Fact]
    public void ASetupThatRunsTwiceWithDifferentInputsIsRejected()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.setup_with_params");

        RunSetup(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make), 5);

        SpecGateCaptureException rejection = Assert.Throws<SpecGateCaptureException>(
            () => RunSetup(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make), 6));

        Assert.Equal(CaptureRejection.SetupAmbiguity, rejection.Rejection);
        Assert.Contains("initial=5 then initial=6", rejection.Message, StringComparison.Ordinal);
    }

    private static RuntimeMethodHandle Handle(Type type, string method) =>
        type.GetMethod(
            method,
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance)!
            .MethodHandle;

    private static void RunSetup(Type type, string method, params object?[] arguments) =>
        SpecGateRuntime.CommitSetup(SpecGateRuntime.StageSetup(Handle(type, method), arguments));

    private static RecordedSpan Operate(
        CaptureSession session,
        Type type,
        string method,
        string operation,
        object?[] arguments,
        object? result = null)
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(
            Handle(type, method), operation, session.Component);
        SpecGateRuntime.RecordInputs(scope, arguments);
        SpecGateRuntime.ExitOk(scope, result);

        return Assert.Single(session.Spans);
    }

    private static SpecGateCaptureException Reject(
        CaptureSession session, Type type, string method, string operation, object?[] arguments)
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(
            Handle(type, method), operation, session.Component);

        return Assert.Throws<SpecGateCaptureException>(
            () => SpecGateRuntime.RecordInputs(scope, arguments));
    }
}
