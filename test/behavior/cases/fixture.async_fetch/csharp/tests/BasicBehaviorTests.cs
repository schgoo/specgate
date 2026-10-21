using SpecGate.CtscFixtures.Conformance.Async;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    [Fact]
    public async Task AsyncFetchRetainsTaskMetadataAndBehavior()
    {
        Assert.Equal("response from https://example.test", await AsyncFetch.Fetch("https://example.test"));
    }
}
