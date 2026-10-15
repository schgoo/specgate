using SpecGate.TestSupport;
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
        using CaptureSession session = CaptureScenario.Begin("fixture.stateless_add");

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", "fixture.multi_case");
        SpecGateRuntime.ExitOk(scope, 5);

        Assert.False(scope.IsRecording);
        Assert.Empty(session.Finish().Operations);
    }

    [Fact]
    public void AMultiComponentOperationRecordsOnlyTheSessionComponent()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.multi_case");
        string[] declared = ["fixture.stateless_add", "fixture.multi_case"];

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", declared);
        SpecGateRuntime.ExitOk(scope, 5);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("fixture.multi_case", span.ComponentId);
        Assert.Equal("add", span.OperationName);
    }

    [Fact]
    public void AMultiComponentOperationMatchingNothingIsNotRecorded()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.other");
        string[] declared = ["fixture.stateless_add", "fixture.multi_case"];

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Add"), "add", declared);
        SpecGateRuntime.ExitOk(scope, 5);

        Assert.False(scope.IsRecording);
        Assert.Empty(session.Finish().Operations);
    }

    [Fact]
    public void AnIgnoredOperationDoesNotDisturbTheSurroundingContext()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.basic");

        OperationScope outer = SpecGateRuntime.EnterOperation(Boundaries.Of("Process"), "process", "fixture.basic");
        OperationScope ignored = SpecGateRuntime.EnterOperation(Boundaries.Of("Process"), "helper", "fixture.other");
        Assert.Same(outer, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(ignored, null);
        Assert.Same(outer, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(outer, "done");

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("process", span.OperationName);
        Assert.Null(SpecGateRuntime.Current);
    }

    [Fact]
    public void ASessionRequiresAComponent()
    {
        Assert.Throws<ArgumentException>(() => CaptureScenario.Begin(string.Empty));
        Assert.False(SpecGateRuntime.IsRecording);
    }
}
