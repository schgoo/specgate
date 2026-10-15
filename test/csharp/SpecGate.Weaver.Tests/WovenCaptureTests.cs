using System;
using System.Linq;
using SpecGate.Runtime;
using SpecGate.TestSupport;
using SpecGate.WeaverFixtures;
using Xunit;

namespace SpecGate.Weaver.Tests;

/// <summary>
/// Executes woven fixtures and asserts on what the capture runtime recorded.
/// Running the fixture at all proves the rewritten IL verifies; an invalid
/// exception handler range or stack shape surfaces as
/// <see cref="InvalidProgramException"/> when the method is first jitted.
/// </summary>
/// <remarks>
/// The behavioral scenario tests here are interim. Once #47 generates C# trace
/// goldens, generated artifacts byte-compared against themselves and compared to
/// the Rust reference under <c>ctsc.strict/0.1.0</c> become the oracle, and #47
/// deletes the scenarios they replace. Only the session-scoping and IL-mechanics
/// tests, which no golden can express, survive that slice.
/// </remarks>
[Collection(WovenCaptureSerialization.Name)]
public sealed class WovenCaptureTests
{
    [Fact]
    public void RecordsInputsAndResultForSimpleOperation()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");

        int sum = Arithmetic.Add(2, 3);

        Assert.Equal(5, sum);
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("weaver.basic", span.ComponentId);
        Assert.Equal("add", span.OperationName);
        Assert.Equal(CaptureScenario.ScenarioSpanId, span.ParentSpanId);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(["a", "b"], span.Inputs.Keys);
        Assert.Equal([Value.Integer(2), Value.Integer(3)], span.Inputs.Values);
        Assert.Equal(Value.Integer(5), span.Result());
    }

    [Fact]
    public void RecordsOnlyTheSessionComponentForMultiComponentOperation()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.alternate");

        Arithmetic.Add(1, 1);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("weaver.alternate", span.ComponentId);
    }

    [Fact]
    public void RecordsNothingForUntargetedComponent()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.absent");

        Arithmetic.Add(1, 1);

        Assert.Empty(session.Finish().Operations);
    }

    [Theory]
    [InlineData(-1, "negative")]
    [InlineData(0, "zero")]
    [InlineData(7, "positive")]
    public void RoutesEveryReturnThroughOneEpilogue(int value, string expected)
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");

        Assert.Equal(expected, Arithmetic.Describe(value));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("describe", span.OperationName);
        Assert.Equal(Value.String(expected), span.Result());
    }

    [Fact]
    public void RecordsThrownExceptionAndRethrows()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");

        Assert.Throws<DivideByZeroException>(() => Arithmetic.Divide(1, 0));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("divide", span.OperationName);
        Assert.Equal(SpanStatus.Error, span.Status);
        FaultCompletion fault = Assert.IsType<FaultCompletion>(span.OperationCompletion);
        Assert.Equal("specgate.unexpected_target_fault", fault.FaultType);
    }

    [Fact]
    public void RecordsVoidSuccessWithoutResult()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");

        Arithmetic.Guard(0);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("guard", span.OperationName);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Null(span.OperationCompletion);
    }

    [Fact]
    public void RecordsVoidFailure()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");

        Assert.Throws<ArgumentOutOfRangeException>(() => Arithmetic.Guard(-1));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Error, span.Status);
        Assert.IsType<FaultCompletion>(span.OperationCompletion);
    }

    [Fact]
    public void RecordsNestedOperationsWithParentage()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");

        Assert.Equal(6, Arithmetic.SumAll([1, 2, 3]));

        // Operations are recorded in start order, so the caller comes first.
        Capture capture = session.Finish();
        OperationSpan outer = capture.Operations[0];
        Assert.Equal("sum_all", outer.OperationName);
        Assert.Equal(CaptureScenario.ScenarioSpanId, outer.ParentSpanId);

        OperationSpan[] inner = [.. capture.Operations.Where(span => span.OperationName == "add")];
        Assert.Equal(3, inner.Length);
        Assert.All(inner, span => Assert.Equal(outer.SpanId, span.ParentSpanId));
    }

    [Theory]
    [InlineData("42", 42)]
    [InlineData("not a number", 0)]
    public void LeavesAnExistingTryCatchIntact(string text, int expected)
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");

        Assert.Equal(expected, Arithmetic.ParseOrDefault(text));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(Value.Integer(expected), span.Result());
        Assert.Equal(
            Value.Tagged("Some", Value.String(text)),
            Assert.Single(span.Inputs).Value);
    }

    [Fact]
    public void SkipsTheImplicitInstanceArgument()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");

        Accumulator accumulator = new();
        Assert.Equal(2, accumulator.Accumulate(2));
        Assert.Equal(5, accumulator.Accumulate(3));

        Capture capture = session.Finish();
        Assert.Equal(2, capture.Operations.Count);
        Assert.All(capture.Operations, span => Assert.Equal("amount", Assert.Single(span.Inputs).Key));
        Assert.Equal(
            [Value.Integer(2), Value.Integer(3)],
            capture.Operations.Select(span => span.Inputs.Values.Single()));
        Assert.Equal(
            [Value.Integer(2), Value.Integer(5)],
            capture.Operations.Select(span => span.Result()));
    }

    [Fact]
    public void RecordsNothingAfterTheSessionEnds()
    {
        CaptureSession session = CaptureScenario.Begin("weaver.basic");
        session.Dispose();

        Assert.Equal(5, Arithmetic.Add(2, 3));
        Assert.Empty(session.Finish().Operations);
    }
}
