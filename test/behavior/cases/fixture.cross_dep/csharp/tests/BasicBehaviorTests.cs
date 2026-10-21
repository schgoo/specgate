using SpecGate.CtscFixtures.Conformance.Dependencies;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    [Fact]
    public void ExternalYamlDependencyParticipatesInTheRealBuild()
    {
        Assert.Equal("SpecGate", ExternalDependency.ParseYamlKey("name: SpecGate", "name"));
        Assert.Equal("null", ExternalDependency.ParseYamlKey("name: SpecGate", "missing"));
    }
}
