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
[Collection(WovenCaptureSerialization.Name)]
public sealed class WovenCaptureTests
{
    [Fact]
    public void IgnoreOnAnIntermediateOverridePreservesSurvivingNativePositions()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");
        bool called = false;
        Assert.Equal(7, new Accumulator.ContextLeaf().Read(() => called = true, 7));
        Assert.True(called);
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(Value.Integer(7), span.Inputs["value"]);
        Assert.Single(span.Inputs);
        Assert.Equal(Value.Integer(7), span.Result());
    }

    [Fact]
    public void IgnoredContextsStillReachTheBodyWithoutBeingProjectedOrShiftingInputs()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.basic");
        int calls = 0;
        Assert.Equal(5, Arithmetic.AddWithContext(2, () => calls++, 3));
        Arithmetic.ContextOnly(() => calls++);
        Assert.Equal(2, calls);

        OperationSpan[] spans = [.. session.Finish().Operations];
        Assert.Equal(2, spans.Length);
        Assert.Equal(["after", "before"], spans[0].Inputs.Keys);
        Assert.Equal([Value.Integer(3), Value.Integer(2)], spans[0].Inputs.Values);
        Assert.Equal(Value.Integer(5), spans[0].Result());
        Assert.Empty(spans[1].Inputs);
        Assert.Null(spans[1].OperationCompletion);
    }

    [Theory]
    [InlineData(0)]
    [InlineData(7)]
    public void DeclaredExceptionsDoNotPeelNormalReturnedWrappers(int state)
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.exception_wrappers");
        _ = Arithmetic.DeclaredOption(state);
        _ = Arithmetic.DeclaredResult(state);
        Assert.Equal(state, Arithmetic.DeclaredScalar(state));
        Arithmetic.DeclaredUnit(state);
        _ = Arithmetic.CatchAllOption(state);

        OperationSpan[] spans = [.. session.Finish().Operations];
        Assert.Equal(5, spans.Length);
        Assert.All(spans, span => Assert.Equal(SpanStatus.Ok, span.Status));
        Assert.Equal(state == 0
            ? Value.Tagged("None", Value.Unit)
            : Value.Tagged("Some", Value.Integer(state)), spans[0].Result());
        Assert.Equal(state == 0
            ? Value.Tagged("Err", Value.Integer(17))
            : Value.Tagged("Ok", Value.Integer(state)), spans[1].Result());
        Assert.Equal(Value.Integer(state), spans[2].Result());
        Assert.Null(spans[3].OperationCompletion);
        Assert.Equal(spans[0].Result(), spans[4].Result());
    }

    [Theory]
    [InlineData(-1)]
    [InlineData(-2)]
    public void OnlyActualDeclaredThrowsUseTheSyntheticErrorChannel(int state)
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.exception_wrappers");
        Assert.ThrowsAny<Exception>(() => Arithmetic.DeclaredOption(state));
        Assert.ThrowsAny<Exception>(() => Arithmetic.DeclaredResult(state));
        Assert.ThrowsAny<Exception>(() => Arithmetic.DeclaredScalar(state));
        Assert.ThrowsAny<Exception>(() => Arithmetic.DeclaredUnit(state));
        Assert.ThrowsAny<Exception>(() => Arithmetic.CatchAllOption(state));

        Capture capture = session.Finish();
        Assert.Equal(5, capture.Operations.Count);
        Assert.All(capture.Operations.Take(4), span =>
        {
            Assert.Equal(SpanStatus.Error, span.Status);
            if (state == -1)
            {
                ErrorCompletion error = Assert.IsType<ErrorCompletion>(span.OperationCompletion);
                Assert.Equal("error", error.Name);
                Assert.Equal(Value.String("declared"), error.ErrorValue);
            }
            else
            {
                Assert.IsType<FaultCompletion>(span.OperationCompletion);
            }
        });
        ErrorCompletion caught = Assert.IsType<ErrorCompletion>(capture.Operations[4].OperationCompletion);
        Assert.Equal("error", caught.Name);
        Assert.Equal(Value.String(state == -1 ? "declared" : "undeclared"), caught.ErrorValue);
    }

    [Fact]
    public void RecordsNothingForUntargetedComponent()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.absent");

        Arithmetic.Describe(1);

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

        Assert.Equal("positive", Arithmetic.Describe(5));
        Assert.Empty(session.Finish().Operations);
    }
}
