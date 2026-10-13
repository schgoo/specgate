using System;
using System.Linq;
using SpecGate.Runtime;
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
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");

        int sum = Arithmetic.Add(2, 3);

        Assert.Equal(5, sum);
        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("weaver.basic", span.Component);
        Assert.Equal("add", span.Operation);
        Assert.Null(span.ParentSpanId);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(["a", "b"], span.Inputs.Select(input => input.Name));
        Assert.Equal([2, 3], span.Inputs.Select(input => input.Value).Cast<int>());
        Assert.Equal(5, Assert.IsType<int>(span.Result?.Value));
    }

    [Fact]
    public void RecordsOnlyTheSessionComponentForMultiComponentOperation()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.alternate");

        Arithmetic.Add(1, 1);

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("weaver.alternate", span.Component);
    }

    [Fact]
    public void RecordsNothingForUntargetedComponent()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.absent");

        Arithmetic.Add(1, 1);

        Assert.Empty(session.Spans);
    }

    [Theory]
    [InlineData(-1, "negative")]
    [InlineData(0, "zero")]
    [InlineData(7, "positive")]
    public void RoutesEveryReturnThroughOneEpilogue(int value, string expected)
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");

        Assert.Equal(expected, Arithmetic.Describe(value));

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("describe", span.Operation);
        Assert.Equal(expected, span.Result?.Value);
    }

    [Fact]
    public void RecordsThrownExceptionAndRethrows()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");

        Assert.Throws<DivideByZeroException>(() => Arithmetic.Divide(1, 0));

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("divide", span.Operation);
        Assert.Equal(SpanStatus.Error, span.Status);
        Assert.Equal("System.DivideByZeroException", span.ErrorType);
        Assert.NotNull(span.ErrorMessage);
        Assert.Null(span.Result);
    }

    [Fact]
    public void RecordsVoidSuccessWithoutResult()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");

        Arithmetic.Guard(0);

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("guard", span.Operation);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Null(span.Result);
    }

    [Fact]
    public void RecordsVoidFailure()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");

        Assert.Throws<ArgumentOutOfRangeException>(() => Arithmetic.Guard(-1));

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal(SpanStatus.Error, span.Status);
        Assert.Equal("System.ArgumentOutOfRangeException", span.ErrorType);
    }

    [Fact]
    public void RecordsNestedOperationsWithParentage()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");

        Assert.Equal(6, Arithmetic.SumAll([1, 2, 3]));

        // Inner spans complete first, so the outer operation is recorded last.
        RecordedSpan outer = session.Spans[^1];
        Assert.Equal("sum_all", outer.Operation);
        Assert.Null(outer.ParentSpanId);

        RecordedSpan[] inner = [.. session.Spans.Where(span => span.Operation == "add")];
        Assert.Equal(3, inner.Length);
        Assert.All(inner, span => Assert.Equal(outer.SpanId, span.ParentSpanId));
    }

    [Theory]
    [InlineData("42", 42)]
    [InlineData("not a number", 0)]
    public void LeavesAnExistingTryCatchIntact(string text, int expected)
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");

        Assert.Equal(expected, Arithmetic.ParseOrDefault(text));

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(expected, span.Result?.Value);
        Assert.Equal(text, Assert.Single(span.Inputs).Value);
    }

    [Fact]
    public void SkipsTheImplicitInstanceArgument()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");

        Accumulator accumulator = new();
        Assert.Equal(2, accumulator.Accumulate(2));
        Assert.Equal(5, accumulator.Accumulate(3));

        Assert.Equal(2, session.Spans.Count);
        Assert.All(session.Spans, span => Assert.Equal("amount", Assert.Single(span.Inputs).Name));
        Assert.Equal([2, 3], session.Spans.Select(span => span.Inputs[0].Value).Cast<int>());
        Assert.Equal([2, 5], session.Spans.Select(span => span.Result?.Value).Cast<int>());
    }

    [Fact]
    public void RecordsNothingAfterTheSessionEnds()
    {
        CaptureSession session = SpecGateRuntime.BeginSession("weaver.basic");
        session.Dispose();

        Assert.Equal(5, Arithmetic.Add(2, 3));
        Assert.Empty(session.Spans);
    }
}
