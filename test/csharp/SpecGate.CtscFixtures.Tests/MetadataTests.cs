using System.Reflection;
using SpecGate.Annotations;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class MetadataTests
{
    [Fact]
    public void StatelessOperationCarriesSemanticMetadata()
    {
        MethodInfo method = typeof(Stateless).GetMethod(nameof(Stateless.Add))!;
        SpecOperationAttribute operation = Assert.Single(method.GetCustomAttributes<SpecOperationAttribute>());
        Assert.Equal("add", operation.Name);
        Assert.Equal("fixture.stateless_add", operation.Spec);
        Assert.Equal(5, Stateless.Add(2, 3));
    }

    [Fact]
    public void RichAndSetupTypesCarryDiscoveryMetadata()
    {
        Assert.NotNull(typeof(Person).GetCustomAttribute<SpecEventAttribute>());
        MethodInfo setup = typeof(Counter).GetMethod(nameof(Counter.Make))!;
        SpecSetupAttribute attribute = Assert.Single(setup.GetCustomAttributes<SpecSetupAttribute>());
        Assert.Equal("fixture.setup", attribute.Spec);
    }

    [Fact]
    public void UnscopedSetupAndFallibleUnitMetadataRemainAvailable()
    {
        MethodInfo setup = typeof(UnitCounter).GetMethod(nameof(UnitCounter.Make))!;
        SpecSetupAttribute setupAttribute = Assert.Single(setup.GetCustomAttributes<SpecSetupAttribute>());
        Assert.Null(setupAttribute.Spec);

        MethodInfo task = typeof(FallibleUnit).GetMethod(nameof(FallibleUnit.FallibleTask))!;
        Assert.Equal(typeof(Task), task.ReturnType);
        SpecExceptionAttribute exception = Assert.Single(task.GetCustomAttributes<SpecExceptionAttribute>());
        Assert.Equal(typeof(InvalidOperationException), Assert.Single(exception.ExceptionTypes));
    }
}
