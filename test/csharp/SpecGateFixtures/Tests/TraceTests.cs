using SpecGate.Runtime;
using Xunit;

namespace SpecGateFixtures.Tests;

public class TraceTests
{
    [Fact]
    public void Add_2_3_EmitsCanonicalTraceJson()
    {
        SpecGateRuntime.Reset();
        SpecGateRuntime.EmitRun("add");
        int a = 2, b = 3;
        SpecGateRuntime.EmitEvent("add.a", a);
        SpecGateRuntime.EmitEvent("add.b", b);
        int result = StatelessAdd.Add(a, b);
        SpecGateRuntime.EmitEvent("$result", result);
        string json = SpecGateRuntime.GetTracesJson();
        const string expected =
            "[{\"kind\":\"Run\",\"operation\":\"add\"}"
            + ",{\"kind\":\"Event\",\"name\":\"add.a\",\"value\":2}"
            + ",{\"kind\":\"Event\",\"name\":\"add.b\",\"value\":3}"
            + ",{\"kind\":\"Event\",\"name\":\"$result\",\"value\":5}]";
        Assert.Equal(expected, json);
    }

    [Fact]
    public void ScalarValues_SerializeLikeRustValue()
    {
        SpecGateRuntime.Reset();
        SpecGateRuntime.EmitEvent("int", 2);
        SpecGateRuntime.EmitEvent("string", "two");

        const string expected =
            "[{\"kind\":\"Event\",\"name\":\"int\",\"value\":2}"
            + ",{\"kind\":\"Event\",\"name\":\"string\",\"value\":\"two\"}]";
        Assert.Equal(expected, SpecGateRuntime.GetTracesJson());
    }

    [Fact]
    public void StructuredValues_SerializeWithCanonicalOrdering()
    {
        SpecGateRuntime.Reset();
        SpecGateRuntime.EmitResult(ComplexValueFixtures.GetProduct());

        const string expected =
            "[{\"kind\":\"Event\",\"name\":\"product_name\",\"value\":\"Milk\"}"
            + ",{\"kind\":\"Event\",\"name\":\"price\",\"value\":4}"
            + ",{\"kind\":\"Event\",\"name\":\"tags\",\"value\":[\"dairy\",\"organic\",\"local\"]}"
            + ",{\"kind\":\"Event\",\"name\":\"attributes\",\"value\":{\"category\":\"food\",\"origin\":\"local\"}}"
            + ",{\"kind\":\"Event\",\"name\":\"$result\",\"value\":{\"attributes\":{\"category\":\"food\",\"origin\":\"local\"},\"price\":4,\"product_name\":\"Milk\",\"tags\":[\"dairy\",\"organic\",\"local\"]}}]";
        Assert.Equal(expected, SpecGateRuntime.GetTracesJson());
    }

    [Fact]
    public void Sets_SerializeSortedAndDeduplicated()
    {
        SpecGateRuntime.Reset();
        SpecGateRuntime.EmitResult(new HashSet<string> { "Orders", "Address", "Contacts", "Orders" });

        const string expected =
            "[{\"kind\":\"Event\",\"name\":\"$result\",\"value\":[\"Address\",\"Contacts\",\"Orders\"]}]";
        Assert.Equal(expected, SpecGateRuntime.GetTracesJson());
    }

    [Fact]
    public void SumTypes_SerializeAsTaggedMaps()
    {
        SpecGateRuntime.Reset();
        SpecGateRuntime.EmitResult(new Circle { Radius = 5.0 });
        SpecGateRuntime.EmitResult(Option<int>.Some(1));
        SpecGateRuntime.EmitResult(Option<int>.None());
        SpecGateRuntime.EmitResult(Result<int, string>.Ok(5));
        SpecGateRuntime.EmitResult(Result<int, string>.Err("division by zero"));

        const string expected =
            "[{\"kind\":\"Event\",\"name\":\"$result\",\"value\":{\"Circle\":{\"radius\":5}}}"
            + ",{\"kind\":\"Event\",\"name\":\"$result\",\"value\":{\"Some\":1}}"
            + ",{\"kind\":\"Event\",\"name\":\"$result\",\"value\":{\"None\":{}}}"
            + ",{\"kind\":\"Event\",\"name\":\"$result\",\"value\":{\"Ok\":5}}"
            + ",{\"kind\":\"Event\",\"name\":\"$result\",\"value\":{\"Err\":\"division by zero\"}}]";
        Assert.Equal(expected, SpecGateRuntime.GetTracesJson());
    }
}
