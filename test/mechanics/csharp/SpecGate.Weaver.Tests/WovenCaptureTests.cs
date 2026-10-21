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
