using SpecGate.TestSupport;
using Xunit;

namespace SpecGate.Runtime.Tests;

[Collection(nameof(CaptureSessionSerialization))]
public sealed class CaptureSessionTests
{
    [Fact]
    public void OperationsRecordNothingWithoutASession()
    {
        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.basic");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        SpecGateRuntime.ExitOk(scope, 5);

        Assert.False(scope.IsRecording);
        Assert.Null(scope.SpanId);
        Assert.False(SpecGateRuntime.IsRecording);
        Assert.Null(SpecGateRuntime.Current);
    }

    [Fact]
    public void ASecondConcurrentSessionIsRejected()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        InvalidOperationException error = Assert.Throws<InvalidOperationException>(
            () => CaptureScenario.Begin("fixture.basic"));

        Assert.Contains("capture session is already active", error.Message);
    }

    [Fact]
    public void ASuccessfulOperationRecordsInputsAndResult()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.basic");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        SpecGateRuntime.ExitOk(scope, 5);

        Capture capture = session.Finish();
        OperationSpan span = Assert.Single(capture.Operations);
        Assert.Equal("fixture.basic", span.ComponentId);
        Assert.Equal("add", span.OperationName);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(CaptureScenario.ScenarioSpanId, span.ParentSpanId);
        Assert.Equal(
            [new KeyValuePair<string, Value>("left", Value.Integer(2)), new("right", Value.Integer(3))],
            span.OrderedInputs());
        Assert.Equal(Value.Integer(5), span.Result());
    }

    /// <summary>
    /// The logical clock and the event order counter are what make a capture
    /// comparable: two runs of one scenario line up only because every boundary
    /// consumes the same ticks in the same sequence.
    /// </summary>
    [Fact]
    public void TheLogicalClockAndEventOrderAreDeterministic()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.basic");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        SpecGateRuntime.ExitOk(scope, 5);

        Capture capture = session.Finish();
        OperationSpan span = Assert.Single(capture.Operations);

        Assert.Equal(0, capture.Run.StartTimeUnixNano);
        Assert.Equal(1, capture.Scenario.StartTimeUnixNano);
        Assert.Equal(2, span.StartTimeUnixNano);
        Assert.Equal(0u, span.Order);
        Completion completion = Assert.IsType<ResultCompletion>(span.OperationCompletion);
        Assert.Equal(1u, completion.Order);
        Assert.Equal(3, completion.TimeUnixNano);
        Assert.Equal(4, span.EndTimeUnixNano);
        Assert.Equal(5, capture.Scenario.EndTimeUnixNano);
        Assert.Equal(6, capture.Run.EndTimeUnixNano);
        Assert.Null(capture.Run.ParentSpanId);
        Assert.Equal(capture.Run.SpanId, capture.Scenario.ParentSpanId);
    }

    /// <summary>
    /// A void operation has no value channel, so it reports no completion event
    /// at all and consumes no event order.
    /// </summary>
    [Fact]
    public void AVoidOperationReportsNoCompletion()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Reset"), "reset", "fixture.basic");
        SpecGateRuntime.ExitVoid(scope);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Null(span.OperationCompletion);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(2, span.StartTimeUnixNano);
        Assert.Equal(3, span.EndTimeUnixNano);
    }

    /// <summary>
    /// An exception the operation does not declare is not part of its contract,
    /// so it is recorded as an unexpected target fault rather than an error
    /// outcome a candidate could be expected to reproduce.
    /// </summary>
    [Fact]
    public void AnUndeclaredExceptionIsRecordedAsAFault()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Divide"), "divide", "fixture.basic");
        SpecGateRuntime.ExitError(scope, new DivideByZeroException("denominator was zero"));

        Capture capture = session.Finish();
        OperationSpan span = Assert.Single(capture.Operations);
        FaultCompletion fault = Assert.IsType<FaultCompletion>(span.OperationCompletion);
        Assert.Equal("specgate.unexpected_target_fault", fault.FaultType);
        Assert.Equal("operation unwound before returning", fault.Message);
        Assert.Equal("target", fault.Observer);
        Assert.Equal(SpanStatus.Error, span.Status);
        Assert.Equal(SpanStatus.Error, capture.Run.Status);
        Assert.Equal(SpanStatus.Error, capture.Scenario.Status);
        Assert.Null(SpecGateRuntime.Current);
    }

    /// <summary>
    /// An exception listed by <c>[SpecException]</c> is the operation's declared
    /// error outcome, so it fills the error channel the registry publishes for
    /// it instead of being reported as a fault.
    /// </summary>
    [Fact]
    public void ADeclaredExceptionIsRecordedAsADeclaredError()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(
            Boundaries.Of("Checked"), "checked", "fixture.basic");
        SpecGateRuntime.ExitError(scope, new InvalidOperationException("rejected"));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        ErrorCompletion error = Assert.IsType<ErrorCompletion>(span.OperationCompletion);
        Assert.Equal("error", error.Name);
        Assert.Equal(Value.String("rejected"), error.ErrorValue);
        Assert.Equal(SpanStatus.Error, span.Status);
    }

    /// <summary>An exception outside the declared list is still a fault.</summary>
    [Fact]
    public void AnExceptionOutsideTheDeclaredListIsStillAFault()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(
            Boundaries.Of("Checked"), "checked", "fixture.basic");
        SpecGateRuntime.ExitError(scope, new DivideByZeroException("unrelated"));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.IsType<FaultCompletion>(span.OperationCompletion);
    }

    /// <summary>
    /// <c>[SpecException]</c> with no types declares that any exception is the
    /// operation's error outcome.
    /// </summary>
    [Fact]
    public void AnUntypedDeclarationAcceptsAnyException()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(
            Boundaries.Of("CatchAll"), "catch_all", "fixture.basic");
        SpecGateRuntime.ExitError(scope, new DivideByZeroException("anything"));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        ErrorCompletion error = Assert.IsType<ErrorCompletion>(span.OperationCompletion);
        Assert.Equal(Value.String("anything"), error.ErrorValue);
    }

    [Fact]
    public void ARejectedInputLeavesNoOperationCurrent()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope rejected = SpecGateRuntime.EnterOperation(
            Boundaries.Of("Add"), "add", "fixture.basic");
        Assert.Throws<SpecGateCaptureException>(
            () => SpecGateRuntime.RecordInputs(rejected, ["not an int", 3]));

        // The woven try region starts after RecordInputs, so nothing closes this
        // scope. A caller that catches the rejection and calls another operation
        // must not have that operation parented to the abandoned one.
        Assert.Null(SpecGateRuntime.Current);

        OperationScope next = SpecGateRuntime.EnterOperation(
            Boundaries.Of("Add"), "add", "fixture.basic");
        SpecGateRuntime.RecordInputs(next, [2, 3]);
        SpecGateRuntime.ExitOk(next, 5);

        // The abandoned scope never completed, so finishing reports it; the
        // second operation still parents to the scenario rather than to it.
        InvalidOperationException error = Assert.Throws<InvalidOperationException>(session.Finish);
        Assert.Contains("add", error.Message);
        Assert.Null(SpecGateRuntime.Current);
    }

    /// <summary>
    /// A span that never reached a terminal channel is evidence of a broken
    /// capture, not an absence. Finishing records it as an incomplete-capture
    /// fault naming the boundary that stayed open, then fails.
    /// </summary>
    [Fact]
    public void AnOperationLeftOpenFailsTheCapture()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.basic");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        Assert.Throws<SpecGateCaptureException>(() => SpecGateRuntime.ExitOk(scope, "not an int"));

        Assert.Null(SpecGateRuntime.Current);
        InvalidOperationException error = Assert.Throws<InvalidOperationException>(session.Finish);
        Assert.Contains("add", error.Message);
    }

    /// <summary>
    /// Supplied operation span identifiers are a claim about how many operations
    /// the scenario runs, so leaving one unused is a configuration failure rather
    /// than a silently shorter capture.
    /// </summary>
    [Fact]
    public void UnusedOperationSpanIdsFailTheCapture()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession(
            "fixture.basic",
            CaptureScenario.Config(operationSpanIds: ["00000000000000aa", "00000000000000bb"]));

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.basic");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        SpecGateRuntime.ExitOk(scope, 5);

        Assert.Throws<InvalidOperationException>(session.Finish);
    }

    [Fact]
    public void SuppliedOperationSpanIdsAreHandedOutInStartOrder()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession(
            "fixture.basic",
            CaptureScenario.Config(operationSpanIds: ["00000000000000aa", "00000000000000bb"]));

        OperationScope outer = SpecGateRuntime.EnterOperation(
            Boundaries.Of("Process"), "process", "fixture.basic");
        OperationScope inner = SpecGateRuntime.EnterOperation(
            Boundaries.Of("Process"), "upper", "fixture.basic");
        SpecGateRuntime.ExitOk(inner, "HELLO");
        SpecGateRuntime.ExitOk(outer, "HELLO");

        Capture capture = session.Finish();
        Assert.Equal(SpanId.Parse("00000000000000aa"), capture.Operation("process").SpanId);
        Assert.Equal(SpanId.Parse("00000000000000bb"), capture.Operation("upper").SpanId);
    }

    /// <summary>
    /// Operations are recorded in the order they start, not the order they
    /// finish, so a nested operation follows its caller in the capture.
    /// </summary>
    [Fact]
    public void ANestedOperationRecordsItsCallerAsParent()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope outer = SpecGateRuntime.EnterOperation(Boundaries.Of("Process"), "process", "fixture.basic");
        OperationScope inner = SpecGateRuntime.EnterOperation(Boundaries.Of("Process"), "upper", "fixture.basic");
        Assert.Same(inner, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(inner, "HELLO");
        Assert.Same(outer, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(outer, "HELLO");

        Capture capture = session.Finish();
        Assert.Equal(["process", "upper"], capture.Operations.Select(span => span.OperationName));
        OperationSpan outerSpan = capture.Operation("process");
        OperationSpan innerSpan = capture.Operation("upper");
        Assert.Equal(outerSpan.SpanId, innerSpan.ParentSpanId);
        Assert.Equal(CaptureScenario.ScenarioSpanId, outerSpan.ParentSpanId);
        Assert.Null(SpecGateRuntime.Current);
    }

    [Fact]
    public void ASessionCannotBeFinishedTwice()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        session.Finish();

        Assert.Throws<InvalidOperationException>(session.Finish);
    }
}
