using SpecGate.Annotations;
using SpecGate.CtscFixtures.Conformance.Translation.OrderPricing;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class OrderPricingBehaviorTests
{
    [Fact]
    public void StandardDomesticOrderIncludesShippingAndTax()
    {
        PriceQuote quote =
            OrderPricing.PriceOrder(
                [Line("widget", 2, 2500), Line("cable", 3, 500)],
                new Standard(),
                new Domestic(),
                Option<string>.None());

        AssertQuote(quote, 6500, 0, 750, 520, 7770);
    }

    [Fact]
    public void DiscountsCouponsAndRegionsPreserveTranslationSemantics()
    {
        PriceQuote gold =
            OrderPricing.PriceOrder(
                [Line("bulk-pack", 10, 1000), Line("premium-part", 2, 2500)],
                new Gold(),
                new Domestic(),
                Option<string>.Some("SAVE500"));
        AssertQuote(gold, 15_000, 2900, 0, 968, 13_068);

        PriceQuote international =
            OrderPricing.PriceOrder(
                [Line("export-item", 4, 1000)],
                new Silver(),
                new International(),
                Option<string>.Some("SHIPFREE"));
        AssertQuote(international, 4000, 200, 0, 0, 3800);

        PriceQuote threshold =
            OrderPricing.PriceOrder(
                [Line("threshold-item", 1, 10_501)],
                new Gold(),
                new Domestic(),
                Option<string>.None());
        AssertQuote(threshold, 10_501, 1050, 750, 756, 10_957);

        PriceQuote paidInternational =
            OrderPricing.PriceOrder(
                [Line("international-item", 2, 1000)],
                new Standard(),
                new International(),
                Option<string>.None());
        AssertQuote(paidInternational, 2000, 0, 2500, 0, 4500);
    }

    [Fact]
    public void OverflowUsesTheStableFaultMessage()
    {
        OverflowException error =
            Assert.Throws<OverflowException>(
                () =>
                    OrderPricing.PriceOrder(
                        [Line("overflow-item", 2, long.MaxValue)],
                        new Standard(),
                        new Domestic(),
                        Option<string>.None()));
        Assert.Equal("order price overflow", error.Message);
    }

    private static OrderLine Line(string sku, int quantity, long unitPriceCents) =>
        new()
        {
            Sku = sku,
            Quantity = quantity,
            UnitPriceCents = unitPriceCents,
        };

    private static void AssertQuote(
        PriceQuote quote,
        long subtotal,
        long discount,
        long shipping,
        long tax,
        long total)
    {
        Assert.Equal(subtotal, quote.SubtotalCents);
        Assert.Equal(discount, quote.DiscountCents);
        Assert.Equal(shipping, quote.ShippingCents);
        Assert.Equal(tax, quote.TaxCents);
        Assert.Equal(total, quote.TotalCents);
    }
}
