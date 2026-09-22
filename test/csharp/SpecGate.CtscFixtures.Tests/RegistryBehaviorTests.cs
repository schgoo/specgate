using SpecGate.Annotations;
using Xunit;
using Ownership = SpecGate.CtscFixtures.Registry.Components.ComponentOwnership;
using Fallible = SpecGate.CtscFixtures.Registry.Focused.FallibleUnit;
using Rich = SpecGate.CtscFixtures.Registry.Focused.Rich;
using Setup = SpecGate.CtscFixtures.Registry.Focused.Setup;
using Witness = SpecGate.CtscFixtures.Conformance.Witness.DivergenceWitness;

namespace SpecGate.CtscFixtures.Tests;

public sealed class RegistryBehaviorTests
{
    [Fact]
    public void FocusedRichAndSetupFixturesRetainBehavior()
    {
        Rich.Person person = new() { Name = "Ada", Tags = ["engineer"] };
        Option<Rich.Shape> fallback = Option<Rich.Shape>.Some(new Rich.Point());
        Assert.Equal("Ada", Rich.Rich.Describe(person, fallback).Value);
        Assert.False(Rich.Rich.Describe(person, Option<Rich.Shape>.None()).HasValue);
        Assert.Equal(1, Rich.Rich.TagCount(person.Tags));

        Setup.Counter counter = Setup.Counter.Make(7);
        counter.Increment();
        Assert.Equal(8, counter.Count);
        Assert.Equal(8, Setup.Counter.ObserveCount(counter.Count));
    }

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

    [Fact]
    public void ComponentOwnershipAndDivergenceWitnessAreExplicit()
    {
        Assert.Equal("widget", Ownership.MakeWidget().Label);
        Assert.Equal("assembled", Ownership.Assemble().Label);

        Conformance.Witness.EngineInfo info = Witness.EngineInfo();
        Assert.Equal(10, info.Value);
        Assert.Equal("csharp", info.Engine);
    }
}
