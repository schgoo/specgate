using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// The diagnostic rendering of a value, which must read the same as the Rust
/// runtime's so a C# reference and a Rust candidate describe a difference
/// identically.
/// </summary>
public sealed class ValueRenderingTests
{
    [Fact]
    public void ATopLevelStringIsUnquoted() => Assert.Equal("hello", Value.String("hello").ToString());

    [Fact]
    public void AListRendersWithoutSpaces() =>
        Assert.Equal("[1,2]", Value.List([Value.Integer(1), Value.Integer(2)]).ToString());

    [Fact]
    public void ASetRendersWithTheSameBracketsAsAList() =>
        Assert.Equal("[\"a\",\"b\"]", Value.Set([Value.String("a"), Value.String("b")]).ToString());

    [Fact]
    public void AMapQuotesItsKeys() =>
        Assert.Equal(
            "{\"count\":2,\"name\":\"ada\"}",
            Value.Map(
            [
                new KeyValuePair<string, Value>("name", Value.String("ada")),
                new KeyValuePair<string, Value>("count", Value.Integer(2)),
            ]).ToString());

    [Fact]
    public void AStringInsideAContainerIsQuoted() =>
        Assert.Equal("[\"hello\"]", Value.List([Value.String("hello")]).ToString());

    [Fact]
    public void AVariantRendersAsASingleEntryMap() =>
        Assert.Equal("{\"Some\":1}", Value.Tagged("Some", Value.Integer(1)).ToString());
}
