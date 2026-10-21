using Xunit;
using Fallible = SpecGate.CtscFixtures.Registry.Focused.FallibleUnit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class RegistryBehaviorTests
{
    [Fact]
    public async Task FallibleUnitSurfacesRetainUnitAndErrorBehavior()
    {
        Fallible.UnitCounter counter = Fallible.UnitCounter.Make(7);
        counter.Advance();
        Assert.Equal(8, counter.Count);

        Fallible.FallibleUnit.FallibleVoid(false);
        Assert.Equal(
            "failed",
            Assert.Throws<InvalidOperationException>(
                () => Fallible.FallibleUnit.FallibleVoid(true)).Message);
        await Fallible.FallibleUnit.FallibleTask(false);
        Assert.Equal(
            "failed",
            (await Assert.ThrowsAsync<InvalidOperationException>(
                () => Fallible.FallibleUnit.FallibleTask(true))).Message);
    }
}
