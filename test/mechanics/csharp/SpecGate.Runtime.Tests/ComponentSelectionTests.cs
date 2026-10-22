using SpecGate.TestSupport;
using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// A session records exactly one component. Operation and component are 1:1, so
/// entry records an operation when its component matches the session and
/// ignores it otherwise.
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
