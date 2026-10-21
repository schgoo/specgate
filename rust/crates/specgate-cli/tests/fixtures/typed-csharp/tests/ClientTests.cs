using Xunit;

namespace SpecGate.TypedClient.Tests;

public sealed class ClientTests
{
    [Fact]
    public async Task FindsTypedItemsWithAConfiguredClient()
    {
        var client = EnvironmentSetup.MakeClient("""[{"value":7},{"value":11}]""");
        var offset = EnvironmentSetup.MakeOffset(3);
        Assert.Equal(7, (await client.FindByTypeNameAsync<Item>("Item", offset))[0].Value);
        Assert.Equal(2, (await client.FindByTypeNameAsync<Item>("Item", offset)).Count);
        Assert.Equal(2, client.Calls());
    }
}
