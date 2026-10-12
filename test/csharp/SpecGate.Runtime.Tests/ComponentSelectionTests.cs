using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// A session records exactly one component. A method may declare the same
/// operation for several components, so entry selects the declaration matching
/// the session and ignores the rest.
/// </summary>
[Collection(nameof(CaptureSessionSerialization))]
public sealed class ComponentSelectionTests
{
    [Fact]
    public void AnOperationDeclaredForAnotherComponentIsNotRecorded()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.stateless_add");

        OperationScope scope = SpecGateRuntime.EnterOperation("add", "fixture.multi_case");
        SpecGateRuntime.ExitOk(scope, 5);

        Assert.False(scope.IsRecording);
        Assert.Empty(session.Spans);
    }

    [Fact]
    public void AMultiComponentOperationRecordsOnlyTheSessionComponent()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.multi_case");
        string[] declared = ["fixture.stateless_add", "fixture.multi_case"];

        OperationScope scope = SpecGateRuntime.EnterOperation("add", declared);
        SpecGateRuntime.ExitOk(scope, 5);

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("fixture.multi_case", span.Component);
        Assert.Equal("add", span.Operation);
    }

    [Fact]
    public void AMultiComponentOperationMatchingNothingIsNotRecorded()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.other");
        string[] declared = ["fixture.stateless_add", "fixture.multi_case"];

        OperationScope scope = SpecGateRuntime.EnterOperation("add", declared);
        SpecGateRuntime.ExitOk(scope, 5);

        Assert.False(scope.IsRecording);
        Assert.Empty(session.Spans);
    }

    [Fact]
    public void AnIgnoredOperationDoesNotDisturbTheSurroundingContext()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.basic");

        OperationScope outer = SpecGateRuntime.EnterOperation("process", "fixture.basic");
        OperationScope ignored = SpecGateRuntime.EnterOperation("helper", "fixture.other");
        Assert.Same(outer, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(ignored, null);
        Assert.Same(outer, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(outer, "done");

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("process", span.Operation);
        Assert.Null(SpecGateRuntime.Current);
    }

    [Fact]
    public void ASessionRequiresAComponent()
    {
        Assert.Throws<ArgumentException>(() => SpecGateRuntime.BeginSession(string.Empty));
        Assert.False(SpecGateRuntime.IsRecording);
    }
}
