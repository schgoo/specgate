using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Translation.OrderPricing;

[SpecEvent("OrderLine")]
public sealed class OrderLine
{
    [SpecEvent("sku")]
    public string Sku { get; set; } = string.Empty;

    [SpecEvent("quantity")]
    public int Quantity { get; set; }

    [SpecEvent("unit_price_cents")]
    public long UnitPriceCents { get; set; }
}

[SpecEvent("CustomerTier")]
public abstract class CustomerTier;

[SpecEvent("Standard")]
public sealed class Standard : CustomerTier;

[SpecEvent("Silver")]
public sealed class Silver : CustomerTier;

[SpecEvent("Gold")]
public sealed class Gold : CustomerTier;

[SpecEvent("ShippingRegion")]
public abstract class ShippingRegion;

[SpecEvent("Domestic")]
public sealed class Domestic : ShippingRegion;

[SpecEvent("International")]
public sealed class International : ShippingRegion;

[SpecEvent("PriceQuote")]
public sealed class PriceQuote
{
    [SpecEvent("subtotal_cents")]
    public long SubtotalCents { get; set; }

    [SpecEvent("discount_cents")]
    public long DiscountCents { get; set; }

    [SpecEvent("shipping_cents")]
    public long ShippingCents { get; set; }

    [SpecEvent("tax_cents")]
    public long TaxCents { get; set; }

    [SpecEvent("total_cents")]
    public long TotalCents { get; set; }
}

public static class OrderPricing
{
    private const int BulkDiscountThreshold = 10;
    private const long BulkDiscountPercent = 10;
    private const long SilverDiscountPercent = 5;
    private const long GoldDiscountPercent = 10;
    private const long CouponSaveCap = 500;
    private const long FreeShippingThresholdCents = 10_000;
    private const long DomesticShippingCents = 750;
    private const long InternationalShippingCents = 2500;
    private const long DomesticTaxPercent = 8;

    [SpecOperation("price_order", Spec = "fixture.order_pricing_translation")]
    public static PriceQuote PriceOrder(
        [SpecInput("lines")] List<OrderLine> lines,
        [SpecInput("customer_tier")] CustomerTier customerTier,
        [SpecInput("shipping_region")] ShippingRegion shippingRegion,
        [SpecInput("coupon")] Option<string> coupon)
    {
        try
        {
            return PriceOrderChecked(lines, customerTier, shippingRegion, coupon);
        }
        catch (OverflowException)
        {
            throw new OverflowException("order price overflow");
        }
    }

    private static PriceQuote PriceOrderChecked(
        List<OrderLine> lines,
        CustomerTier customerTier,
        ShippingRegion shippingRegion,
        Option<string> coupon)
    {
        checked
        {
            long subtotalCents = 0;
            long bulkDiscountCents = 0;
            foreach (OrderLine line in lines)
            {
                long lineTotal = line.Quantity * line.UnitPriceCents;
                subtotalCents += lineTotal;
                if (line.Quantity >= BulkDiscountThreshold)
                {
                    bulkDiscountCents += lineTotal * BulkDiscountPercent / 100;
                }
            }

            long merchandiseCents = subtotalCents - bulkDiscountCents;
            long tierDiscountPercent = customerTier switch
            {
                Silver => SilverDiscountPercent,
                Gold => GoldDiscountPercent,
                _ => 0,
            };
            long tierDiscountCents = merchandiseCents * tierDiscountPercent / 100;
            merchandiseCents -= tierDiscountCents;

            string? couponValue = coupon.HasValue ? coupon.Value : null;
            long couponDiscountCents =
                couponValue == "SAVE500" ? Math.Min(CouponSaveCap, merchandiseCents) : 0;
            merchandiseCents -= couponDiscountCents;

            long discountCents =
                bulkDiscountCents + tierDiscountCents + couponDiscountCents;
            long shippingCents =
                couponValue == "SHIPFREE" || merchandiseCents >= FreeShippingThresholdCents
                    ? 0
                    : shippingRegion is Domestic
                        ? DomesticShippingCents
                        : InternationalShippingCents;
            long taxCents =
                shippingRegion is Domestic
                    ? merchandiseCents * DomesticTaxPercent / 100
                    : 0;
            long totalCents = merchandiseCents + shippingCents + taxCents;

            return new PriceQuote
            {
                SubtotalCents = subtotalCents,
                DiscountCents = discountCents,
                ShippingCents = shippingCents,
                TaxCents = taxCents,
                TotalCents = totalCents,
            };
        }
    }
}
