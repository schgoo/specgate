using Xunit;
using SetupWithParams = SpecGate.CtscFixtures.Conformance.Stateful.SetupWithParams;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    [Fact]
    public void ParameterizedReadonlyStateMachineAndVoidSurfacesRemainBehavioral()
    {
        SetupWithParams.Counter parameterized = SetupWithParams.Counter.Make(9);
        parameterized.Increment();
        Assert.Equal(10, parameterized.Count);
        ParameterizedReadonlyStateMachineAndVoidSurfacesRemainBehavioralStep1();
        ParameterizedReadonlyStateMachineAndVoidSurfacesRemainBehavioralStep2();
        ParameterizedReadonlyStateMachineAndVoidSurfacesRemainBehavioralStep3();
    }
}
