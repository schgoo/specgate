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
        Assert.False(SpecGateRuntime.IsRecording);
        Assert.Null(SpecGateRuntime.Current);
    }

    [Fact]
    public void ASecondConcurrentSessionIsRejected()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.basic");

        InvalidOperationException error = Assert.Throws<InvalidOperationException>(
            () => SpecGateRuntime.BeginSession("fixture.basic"));

        Assert.Contains("capture session is already active", error.Message);
    }

    [Fact]
    public void ASuccessfulOperationRecordsInputsAndResult()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.basic");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        SpecGateRuntime.ExitOk(scope, 5);

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("fixture.basic", span.Component);
        Assert.Equal("add", span.Operation);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Null(span.ParentSpanId);
        Assert.Equal(2, span.Inputs.Count);
        Assert.Equal("left", span.Inputs[0].Name);
        Assert.Equal(Value.Integer(2), span.Inputs[0].Value);
        Assert.Equal("right", span.Inputs[1].Name);
        Assert.Equal(Value.Integer(3), span.Inputs[1].Value);
        Assert.Equal(Value.Integer(5), span.Result?.Value);
    }

    [Fact]
    public void AThrowingOperationRecordsErrorStatus()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Divide"), "divide", "fixture.basic");
        SpecGateRuntime.ExitError(scope, new DivideByZeroException("denominator was zero"));

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal(SpanStatus.Error, span.Status);
        Assert.Equal("System.DivideByZeroException", span.ErrorType);
        Assert.Equal("denominator was zero", span.ErrorMessage);
        Assert.Null(span.Result);
        Assert.Null(SpecGateRuntime.Current);
    }

    [Fact]
    public void ARejectedInputLeavesNoOperationCurrent()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.basic");

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

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Null(span.ParentSpanId);
        Assert.Null(SpecGateRuntime.Current);
    }

    [Fact]
    public void ARejectedResultLeavesNoOperationCurrent()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.basic");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.basic");
        SpecGateRuntime.RecordInputs(scope, [2, 3]);
        Assert.Throws<SpecGateCaptureException>(() => SpecGateRuntime.ExitOk(scope, "not an int"));

        Assert.Null(SpecGateRuntime.Current);
        Assert.Empty(session.Spans);
    }

    [Fact]
    public void ANestedOperationRecordsItsCallerAsParent()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.basic");

        OperationScope outer = SpecGateRuntime.EnterOperation(Boundaries.Of("Process"), "process", "fixture.basic");
        OperationScope inner = SpecGateRuntime.EnterOperation(Boundaries.Of("Process"), "upper", "fixture.basic");
        Assert.Same(inner, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(inner, "HELLO");
        Assert.Same(outer, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(outer, "HELLO");

        RecordedSpan innerSpan = session.Spans.Single(span => span.Operation == "upper");
        RecordedSpan outerSpan = session.Spans.Single(span => span.Operation == "process");
        Assert.Equal(outerSpan.SpanId, innerSpan.ParentSpanId);
        Assert.Null(outerSpan.ParentSpanId);
        Assert.Null(SpecGateRuntime.Current);
    }
}
