using Xunit;
using MultiMutation = SpecGate.CtscFixtures.Conformance.Stateful.MultiMutation;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    private static void MultiFieldAndRepeatedMutationPreserveIntermediateSemanticsStep1()
    {
        MultiMutation.Counter counter = MultiMutation.Counter.Make();
        counter.IncrementTwice();
        Assert.Equal(2, counter.Count);
        Assert.Equal(1, MultiMutation.Counter.ObserveCount("after_first", 1));
        Assert.Equal(2, MultiMutation.Counter.ObserveCount("after_second", 2));
    }
}
