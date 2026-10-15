using System.Reflection;
using SpecGate.Runtime;
using SpecGate.TestSupport;
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
/// <para>
/// These fixtures are compiled unwoven, so the capture entry points are driven
/// directly with the fixture methods' own handles. That exercises the real
/// assembly scan and the real folding — the woven IL shape is covered separately
/// by the weaver's own fixture.
/// </para>
/// <para>
/// The four facts asserting a folded input surface are an interim surface. A
/// generated C# trace golden asserts the same surface against woven code, so
/// #47 deletes them. The four rejection facts are not interim: a capture bundle
/// only records runs that succeeded, so no golden can express them.
/// </para>
/// </remarks>
public sealed class SetupFoldingTests
{
    [Fact]
    public void AReceiverSetupContributesNoRecordedInput()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.multi_field_capture");

        RunSetup(typeof(MultiField.Account), nameof(MultiField.Account.Make));
        OperationSpan span = Operate(
            session, typeof(MultiField.Account), nameof(MultiField.Account.Withdraw), "withdraw", [25]);

        Assert.Equal(["amount"], span.Inputs.Keys);
        Assert.Equal(Value.Integer(25), span.Inputs.Values.Single());
    }

    [Fact]
    public void ExplicitFillsReplaceTheParametersTheyName()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.multi_setup");

        MultiSetup.Account source = MultiSetup.Transfer.MakeSource();
        MultiSetup.Account target = MultiSetup.Transfer.MakeTarget();
        RunSetup(typeof(MultiSetup.Transfer), nameof(MultiSetup.Transfer.MakeSource));
        RunSetup(typeof(MultiSetup.Transfer), nameof(MultiSetup.Transfer.MakeTarget));

        OperationSpan span = Operate(
            session,
            typeof(MultiSetup.Transfer),
            nameof(MultiSetup.Transfer.Execute),
            "transfer",
            [source, target, 30]);

        Assert.Equal(["amount"], span.Inputs.Keys);
    }

    [Fact]
    public void OneProducerSatisfiesEveryDeclarationItCarries()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.shared_setup");

        Shared.BoxVal box = Shared.Combine.MakeBox();
        RunSetup(typeof(Shared.Combine), nameof(Shared.Combine.MakeBox));

        OperationSpan span = Operate(
            session, typeof(Shared.Combine), nameof(Shared.Combine.Two), "combine", [box, box], 4);

        Assert.Empty(span.Inputs);
    }

    [Fact]
    public void ASetupsOwnInputsBecomeInputsOfTheOperation()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.setup_with_params");

        RunSetup(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make), 5);
        OperationSpan span = Operate(
            session,
            typeof(SetupWithParams.Counter),
            nameof(SetupWithParams.Counter.Increment),
            "increment",
            []);

        Assert.Equal(["initial"], span.Inputs.Keys);
        Assert.Equal(Value.Integer(5), span.Inputs.Values.Single());
    }

    [Fact]
    public void AnOperationWhoseSetupNeverRanIsRejected()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.multi_field_capture");

        SpecGateCaptureException rejection = Reject(
            session, typeof(MultiField.Account), nameof(MultiField.Account.Withdraw), "withdraw", [25]);

        Assert.Equal(CaptureRejection.SetupMissing, rejection.Rejection);
        Assert.Contains("constructing the receiver", rejection.Message, StringComparison.Ordinal);

        // A rejection raised while folding abandons its own scope, so nothing is
        // left current for the caller's next operation to be parented to. The
        // boundary it opened never reached a terminal channel, so the capture
        // itself is no longer trustworthy and refuses to finish.
        Assert.Null(SpecGateRuntime.Current);
        InvalidOperationException failure = Assert.Throws<InvalidOperationException>(session.Finish);
        Assert.Contains("withdraw", failure.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void ASetupThatDoesNotReturnSuccessfullyCommitsNothing()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.setup_with_params");

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
        using CaptureSession session = CaptureScenario.Begin("fixture.setup_with_params");

        RunSetup(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make), 5);
        RunSetup(typeof(SetupWithParams.Counter), nameof(SetupWithParams.Counter.Make), 5);

        OperationSpan span = Operate(
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
        using CaptureSession session = CaptureScenario.Begin("fixture.setup_with_params");

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

    private static OperationSpan Operate(
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

        return Assert.Single(session.Finish().Operations);
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
