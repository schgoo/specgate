using SpecGate.Runtime;
using SpecGateFixtures;
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
}
