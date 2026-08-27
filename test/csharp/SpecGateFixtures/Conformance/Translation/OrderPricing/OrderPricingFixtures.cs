using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Translation.OrderPricing;

/// <summary>A single ordered line item, priced by quantity times unit price.</summary>
[SpecEvent]
public sealed class OrderLine
{
    /// <summary>The line's stock keeping unit.</summary>
    [SpecEvent("sku")]
    public string Sku { get; set; } = string.Empty;

    /// <summary>The number of units ordered.</summary>
    [SpecEvent("quantity")]
    public int Quantity { get; set; }

    /// <summary>The per-unit price, in cents.</summary>
    [SpecEvent("unit_price_cents")]
    public long UnitPriceCents { get; set; }
}

/// <summary>
/// A customer's pricing tier, applied as a percentage discount on top of any
/// bulk per-line discount. Variants serialize as tagged maps keyed by
/// variant name (<c>Standard</c>, <c>Silver</c>, <c>Gold</c>).
/// </summary>
[SpecEvent]
public abstract class CustomerTier
{
}

/// <summary>The default tier: no tier-level discount.</summary>
public sealed class Standard : CustomerTier
{
}

/// <summary>The mid tier: 5% off the post-bulk-discount merchandise total.</summary>
public sealed class Silver : CustomerTier
{
}

/// <summary>The top tier: 10% off the post-bulk-discount merchandise total.</summary>
public sealed class Gold : CustomerTier
{
}

/// <summary>
/// The shipping destination, which selects the flat shipping rate and
/// whether merchandise tax applies. Variants serialize as tagged maps keyed
/// by variant name (<c>Domestic</c>, <c>International</c>).
/// </summary>
[SpecEvent]
public abstract class ShippingRegion
{
}

/// <summary>Domestic shipping: flat 750-cent rate (unless waived) plus 8% merchandise tax.</summary>
public sealed class Domestic : ShippingRegion
{
}

/// <summary>International shipping: flat 2500-cent rate (unless waived) and no merchandise tax.</summary>
public sealed class International : ShippingRegion
{
}

/// <summary>The computed price breakdown for an order.</summary>
[SpecEvent]
public sealed class PriceQuote
{
    /// <summary>The pre-discount sum of quantity times unit price across all lines.</summary>
    [SpecEvent("subtotal_cents")]
    public long SubtotalCents { get; set; }

    /// <summary>The total discount applied (bulk, tier, and coupon combined), in cents.</summary>
    [SpecEvent("discount_cents")]
    public long DiscountCents { get; set; }

    /// <summary>The shipping charge, in cents.</summary>
    [SpecEvent("shipping_cents")]
    public long ShippingCents { get; set; }

    /// <summary>The merchandise tax, in cents.</summary>
    [SpecEvent("tax_cents")]
    public long TaxCents { get; set; }

    /// <summary>The final total: discounted merchandise plus shipping plus tax, in cents.</summary>
    [SpecEvent("total_cents")]
    public long TotalCents { get; set; }
}

/// <summary>
/// Prices an order: sums line totals, applies stacked bulk/tier/coupon
/// discounts, selects a shipping charge, and computes merchandise tax — all
/// in integer cents with truncating division.
/// </summary>
public static class OrderPricingOps
{
    private const int BulkDiscountThreshold = 10;
    private const int BulkDiscountPercent = 10;
    private const int SilverDiscountPercent = 5;
    private const int GoldDiscountPercent = 10;
    private const long CouponSaveCap = 500;
    private const long FreeShippingThresholdCents = 10_000;
    private const long DomesticShippingCents = 750;
    private const long InternationalShippingCents = 2500;
    private const int DomesticTaxPercent = 8;

    /// <summary>Computes a full price breakdown for an order.</summary>
    /// <param name="lines">The ordered line items (spec input <c>lines</c>).</param>
    /// <param name="customerTier">The customer's pricing tier (spec input <c>customer_tier</c>).</param>
    /// <param name="shippingRegion">The shipping destination (spec input <c>shipping_region</c>).</param>
    /// <param name="coupon">An optional coupon code (spec input <c>coupon</c>).</param>
    /// <returns>The computed <see cref="PriceQuote"/>.</returns>
    /// <exception cref="OverflowException">
    /// Thrown with the exact message <c>order price overflow</c> when any
    /// pricing arithmetic overflows a 64-bit signed integer.
    /// </exception>
    [SpecOperation("price_order", Spec = "fixture.order_pricing_translation")]
    public static PriceQuote PriceOrder(
        [SpecInput("lines")] List<OrderLine> lines,
        [SpecInput("customer_tier")] CustomerTier customerTier,
        [SpecInput("shipping_region")] ShippingRegion shippingRegion,
        [SpecInput("coupon")] string? coupon)
    {
        try
        {
            return PriceOrderChecked(lines, customerTier, shippingRegion, coupon);
        }
        catch (OverflowException)
        {
            // Map the platform-specific overflow message to the exact fault
            // string the spec expects, so the harness records a deterministic
            // `$fault: order price overflow` instead of leaking runtime text.
            throw new OverflowException("order price overflow");
        }
    }

    /// <summary>
    /// Runs the pricing algorithm in a checked context so any arithmetic
    /// overflow throws <see cref="OverflowException"/> instead of silently
    /// wrapping. <see cref="PriceOrder"/> is the boundary that maps that
    /// exception to the spec's exact fault message.
    /// </summary>
    private static PriceQuote PriceOrderChecked(
        List<OrderLine> lines,
        CustomerTier customerTier,
        ShippingRegion shippingRegion,
        string? coupon)
    {
        checked
        {
            long subtotalCents = 0;
            long bulkDiscountCents = 0;
            foreach (var line in lines)
            {
                long lineTotal = line.Quantity * line.UnitPriceCents;
                subtotalCents += lineTotal;
                if (line.Quantity >= BulkDiscountThreshold)
                {
                    bulkDiscountCents += lineTotal * BulkDiscountPercent / 100;
                }
            }

            long merchandiseCents = subtotalCents - bulkDiscountCents;

            int tierDiscountPercent = customerTier switch
            {
                Silver => SilverDiscountPercent,
                Gold => GoldDiscountPercent,
                _ => 0,
            };
            long tierDiscountCents = merchandiseCents * tierDiscountPercent / 100;
            merchandiseCents -= tierDiscountCents;

            long couponDiscountCents = coupon switch
            {
                "SAVE500" => Math.Min(CouponSaveCap, merchandiseCents),
                _ => 0,
            };
            merchandiseCents -= couponDiscountCents;

            long discountCents = bulkDiscountCents + tierDiscountCents + couponDiscountCents;

            long shippingCents;
            if (coupon == "SHIPFREE")
            {
                shippingCents = 0;
            }
            else if (merchandiseCents >= FreeShippingThresholdCents)
            {
                shippingCents = 0;
            }
            else
            {
                shippingCents = shippingRegion switch
                {
                    Domestic => DomesticShippingCents,
                    _ => InternationalShippingCents,
                };
            }

            long taxCents = shippingRegion switch
            {
                Domestic => merchandiseCents * DomesticTaxPercent / 100,
                _ => 0,
            };

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
