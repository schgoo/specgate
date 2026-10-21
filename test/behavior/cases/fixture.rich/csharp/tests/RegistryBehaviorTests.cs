using SpecGate.Annotations;
using Xunit;
using Rich = SpecGate.CtscFixtures.Registry.Focused.Rich;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class RegistryBehaviorTests
{
    [Fact]
    public void FocusedRichAndSetupFixturesRetainBehavior()
    {
        Rich.Person person = new() { Name = "Ada", Tags = ["engineer"] };
        Option<Rich.Shape> fallback = Option<Rich.Shape>.Some(new Rich.Point());
        Assert.Equal("Ada", Rich.Rich.Describe(person, fallback).Value);
        Assert.False(Rich.Rich.Describe(person, Option<Rich.Shape>.None()).HasValue);
        Assert.Equal(1, Rich.Rich.TagCount(person.Tags));
        FocusedRichAndSetupFixturesRetainBehaviorStep1();
    }
}
