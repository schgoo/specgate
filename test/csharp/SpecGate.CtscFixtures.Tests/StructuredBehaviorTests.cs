using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class StructuredBehaviorTests
{
    [Fact]
    public void ScalarAndStructuredRecordsRemainDistinct()
    {
        Assert.Equal(
            9_000_000_000L,
            Assert.IsType<IntegerValue>(ScalarTypes.Classify(9_000_000_000, true)).Value);
        Assert.False(
            Assert.IsType<BooleanValue>(ScalarTypes.Classify(9_000_000_000, false)).Value);

        Measurement measurement = ScalarOperators.GetMeasurement();
        Assert.Equal(72, measurement.Temperature);
        Assert.StartsWith("sensor-", measurement.Label, StringComparison.Ordinal);
        Assert.Equal(5, measurement.Readings.Count);
        Assert.Empty(ScalarOperators.GetEmpty());

        Product product = Operators.GetProduct();
        Assert.Equal("Milk", product.Name);
        Assert.Contains("organic", product.Tags);
        Assert.Equal("local", product.Attributes["origin"]);

        EntityType entity = StructuredOutput.ResolveEntity();
        Assert.Equal("Customer", entity.Name);
        Assert.Equal(["ID"], entity.KeyProperties);
        Assert.Equal(["ID", "Name", "Email"], entity.StructuralProperties);
    }

    [Fact]
    public void MapsSetsAndNestedCollectionsAreDeterministic()
    {
        SortedDictionary<string, string> values =
            StructuredCollections.GetEntityValues(12);
        Assert.Equal("12", values["ID"]);
        Assert.Equal("cust@example.com", values["Email"]);
        Assert.Equal(
            ["Address", "Contacts", "Orders"],
            StructuredCollections.GetNavigationProperties());

        List<SortedDictionary<string, string>> properties =
            StructuredCollections.GetProperties();
        Assert.Equal("ID", properties[0]["name"]);
        Assert.Equal("true", properties[1]["nullable"]);
    }

    [Fact]
    public void FormerDefaultInputsAreExplicitOperationInputs()
    {
        Assert.Equal(12, DefaultInput.Scale(6, 2));
        Assert.Equal(12, DefaultInput.Shift(10, new Offset { Dx = 1, Dy = 1 }));
    }
}
