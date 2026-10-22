using Xunit;
using StateMachine = SpecGate.CtscFixtures.Conformance.Stateful.StatemachineCounter;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    private static void ParameterizedReadonlyStateMachineAndVoidSurfacesRemainBehavioralStep2()
    {
        StateMachine.Counter stateMachine = StateMachine.Counter.Make();
        stateMachine.Increment();
        Assert.Equal(1, stateMachine.Count);
        Assert.Equal(1, StateMachine.Counter.ObserveState(stateMachine).Count);
    }
}
