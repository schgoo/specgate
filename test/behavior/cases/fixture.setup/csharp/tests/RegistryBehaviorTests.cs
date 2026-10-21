using Xunit;
using Setup = SpecGate.CtscFixtures.Registry.Focused.Setup;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class RegistryBehaviorTests
{
    private static void FocusedRichAndSetupFixturesRetainBehaviorStep1()
    {
        Setup.Counter counter = Setup.Counter.Make(7);
        counter.Increment();
        Assert.Equal(8, counter.Count);
        Assert.Equal(8, Setup.Counter.ObserveCount(counter.Count));
    }
}
