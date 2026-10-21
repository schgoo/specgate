using System.Net;
using System.Text.Json;
using SpecGate.Annotations;

namespace SpecGate.TypedClient;

[SpecEvent("Item")]
public sealed class Item
{
    [SpecEvent("value")]
    public int Value { get; set; }
}

[SpecEvent("Offset")]
public sealed class Offset
{
    [SpecEvent("value")]
    public int Value { get; set; }
}

[SpecEvent("Client")]
public sealed class Client(HttpClient http)
{
    private int calls;

    [SpecOperation("find_items", Spec = "fixture.typed_async", TypeArguments = new[] { typeof(Item) })]
    public async Task<List<T>> FindByTypeNameAsync<T>(
        [SpecInput("type_name")] string name, Offset offset)
        where T : notnull
    {
        if (name != "Item" || offset.Value != 3)
        {
            throw new ArgumentException("Unexpected recorded client inputs");
        }

        await Task.Yield();
        calls++;
        var response = await http.GetStringAsync("http://fixture/items");
        return JsonSerializer.Deserialize<List<T>>(response, JsonSerializerOptions.Web)
            ?? throw new InvalidOperationException("The setup must supply an item collection");
    }

    [SpecOperation("calls", Spec = "fixture.typed_async")]
    public int Calls() => calls;
}

public static class EnvironmentSetup
{
    [SpecSetup("find_items", Spec = "fixture.typed_async")]
    [SpecSetup("calls", Spec = "fixture.typed_async")]
    public static Client MakeClient([SpecInput("response")] string body) =>
        new(new HttpClient(new RecordedResponse(body)));

    [SpecSetup("find_items", Spec = "fixture.typed_async", Fills = "offset")]
    public static Offset MakeOffset([SpecInput("initial")] int seed) => new() { Value = seed };

    private sealed class RecordedResponse(string body) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) =>
            Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(body) });
    }
}
