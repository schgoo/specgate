using Xunit;
using MultiStep = SpecGate.CtscFixtures.Conformance.Stateful.MultiStep;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    [Fact]
    public void MultiStepAndNestedOperationsShareReceiverState()
    {
        MultiStep.Counter counter = MultiStep.Counter.Make();
        counter.Increment();
        counter.Increment();
        counter.Decrement();
        Assert.Equal(1, counter.Count);
        MultiStepAndNestedOperationsShareReceiverStateStep1();
    }
}
