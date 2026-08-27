// Order pricing: sums line totals, applies stacked bulk/tier/coupon
// discounts, selects a shipping charge, and computes merchandise tax — all
// in integer cents with truncating division. Direct Rust translation of the
// approved C# original at
// `Conformance/Translation/OrderPricing/OrderPricingFixtures.cs`.
use serde::{Deserialize, Serialize};
use specgate::*;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single ordered line item, priced by quantity times unit price.
#[derive(Serialize, Deserialize, SpecEvent)]
#[spec_component("fixture.order_pricing_translation")]
pub struct OrderLine {
    /// The line's stock keeping unit.
    #[spec_event]
    pub sku: String,
    /// The number of units ordered.
    #[spec_event]
    pub quantity: i32,
    /// The per-unit price, in cents.
    #[spec_event]
    pub unit_price_cents: i64,
}

/// A customer's pricing tier, applied as a percentage discount on top of
/// any bulk per-line discount.
#[derive(Serialize, Deserialize, SpecEvent)]
#[spec_component("fixture.order_pricing_translation")]
pub enum CustomerTier {
    /// The default tier: no tier-level discount.
    Standard,
    /// The mid tier: 5% off the post-bulk-discount merchandise total.
    Silver,
    /// The top tier: 10% off the post-bulk-discount merchandise total.
    Gold,
}

/// The shipping destination, which selects the flat shipping rate and
/// whether merchandise tax applies.
#[derive(Serialize, Deserialize, SpecEvent)]
#[spec_component("fixture.order_pricing_translation")]
pub enum ShippingRegion {
    /// Domestic shipping: flat 750-cent rate (unless waived) plus 8%
    /// merchandise tax.
    Domestic,
    /// International shipping: flat 2500-cent rate (unless waived) and no
    /// merchandise tax.
    International,
}

/// The computed price breakdown for an order.
#[derive(Serialize, Deserialize, SpecEvent)]
#[spec_component("fixture.order_pricing_translation")]
pub struct PriceQuote {
    /// The pre-discount sum of quantity times unit price across all lines.
    #[spec_event]
    pub subtotal_cents: i64,
    /// The total discount applied (bulk, tier, and coupon combined), in
    /// cents.
    #[spec_event]
    pub discount_cents: i64,
    /// The shipping charge, in cents.
    #[spec_event]
    pub shipping_cents: i64,
    /// The merchandise tax, in cents.
    #[spec_event]
    pub tax_cents: i64,
    /// The final total: discounted merchandise plus shipping plus tax, in
    /// cents.
    #[spec_event]
    pub total_cents: i64,
}

// ---------------------------------------------------------------------------
// Operation
// ---------------------------------------------------------------------------

const BULK_DISCOUNT_THRESHOLD: i32 = 10;
const BULK_DISCOUNT_PERCENT: i64 = 10;
const SILVER_DISCOUNT_PERCENT: i64 = 5;
const GOLD_DISCOUNT_PERCENT: i64 = 10;
const COUPON_SAVE_CAP: i64 = 500;
const FREE_SHIPPING_THRESHOLD_CENTS: i64 = 10_000;
const DOMESTIC_SHIPPING_CENTS: i64 = 750;
const INTERNATIONAL_SHIPPING_CENTS: i64 = 2500;
const DOMESTIC_TAX_PERCENT: i64 = 8;

fn checked(value: Option<i64>) -> i64 {
    value.unwrap_or_else(|| panic!("order price overflow"))
}

/// Computes a full price breakdown for an order.
#[spec_operation("price_order", spec = "fixture.order_pricing_translation")]
pub fn price_order(
    lines: Vec<OrderLine>,
    customer_tier: CustomerTier,
    shipping_region: ShippingRegion,
    coupon: Option<String>,
) -> PriceQuote {
    let mut subtotal_cents: i64 = 0;
    let mut bulk_discount_cents: i64 = 0;
    for line in &lines {
        let line_total = checked((line.quantity as i64).checked_mul(line.unit_price_cents));
        subtotal_cents = checked(subtotal_cents.checked_add(line_total));
        if line.quantity >= BULK_DISCOUNT_THRESHOLD {
            let bulk_discount = checked(
                line_total
                    .checked_mul(BULK_DISCOUNT_PERCENT)
                    .and_then(|value| value.checked_div(100)),
            );
            bulk_discount_cents = checked(bulk_discount_cents.checked_add(bulk_discount));
        }
    }

    let mut merchandise_cents = checked(subtotal_cents.checked_sub(bulk_discount_cents));

    let tier_discount_percent: i64 = match customer_tier {
        CustomerTier::Silver => SILVER_DISCOUNT_PERCENT,
        CustomerTier::Gold => GOLD_DISCOUNT_PERCENT,
        CustomerTier::Standard => 0,
    };
    let tier_discount_cents = checked(
        merchandise_cents
            .checked_mul(tier_discount_percent)
            .and_then(|value| value.checked_div(100)),
    );
    merchandise_cents = checked(merchandise_cents.checked_sub(tier_discount_cents));

    let coupon_discount_cents: i64 = match coupon.as_deref() {
        Some("SAVE500") => COUPON_SAVE_CAP.min(merchandise_cents),
        _ => 0,
    };
    merchandise_cents = checked(merchandise_cents.checked_sub(coupon_discount_cents));

    let discount_cents = checked(
        bulk_discount_cents
            .checked_add(tier_discount_cents)
            .and_then(|value| value.checked_add(coupon_discount_cents)),
    );

    let shipping_cents: i64 = if coupon.as_deref() == Some("SHIPFREE") {
        0
    } else if merchandise_cents >= FREE_SHIPPING_THRESHOLD_CENTS {
        0
    } else {
        match &shipping_region {
            ShippingRegion::Domestic => DOMESTIC_SHIPPING_CENTS,
            ShippingRegion::International => INTERNATIONAL_SHIPPING_CENTS,
        }
    };

    let tax_cents: i64 = match &shipping_region {
        ShippingRegion::Domestic => checked(
            merchandise_cents
                .checked_mul(DOMESTIC_TAX_PERCENT)
                .and_then(|value| value.checked_div(100)),
        ),
        ShippingRegion::International => 0,
    };

    let total_cents = checked(
        merchandise_cents
            .checked_add(shipping_cents)
            .and_then(|value| value.checked_add(tax_cents)),
    );

    PriceQuote {
        subtotal_cents,
        discount_cents,
        shipping_cents,
        tax_cents,
        total_cents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(sku: &str, quantity: i32, unit_price_cents: i64) -> OrderLine {
        OrderLine {
            sku: sku.to_string(),
            quantity,
            unit_price_cents,
        }
    }

    // Standard pricing pays domestic shipping and 8% merchandise tax.
    #[test]
    fn standard_domestic_order() {
        let quote = price_order(
            vec![line("widget", 2, 2500), line("cable", 3, 500)],
            CustomerTier::Standard,
            ShippingRegion::Domestic,
            None,
        );
        assert_eq!(quote.subtotal_cents, 6500);
        assert_eq!(quote.discount_cents, 0);
        assert_eq!(quote.shipping_cents, 750);
        assert_eq!(quote.tax_cents, 520);
        assert_eq!(quote.total_cents, 7770);
    }

    // Bulk, Gold-tier, and SAVE500 discounts stack before tax.
    #[test]
    fn gold_bulk_order_with_coupon() {
        let quote = price_order(
            vec![line("bulk-pack", 10, 1000), line("premium-part", 2, 2500)],
            CustomerTier::Gold,
            ShippingRegion::Domestic,
            Some("SAVE500".to_string()),
        );
        assert_eq!(quote.subtotal_cents, 15000);
        assert_eq!(quote.discount_cents, 2900);
        assert_eq!(quote.shipping_cents, 0);
        assert_eq!(quote.tax_cents, 968);
        assert_eq!(quote.total_cents, 13068);
    }

    // Silver discount applies, SHIPFREE removes shipping, and international
    // tax is zero.
    #[test]
    fn silver_international_shipfree() {
        let quote = price_order(
            vec![line("export-item", 4, 1000)],
            CustomerTier::Silver,
            ShippingRegion::International,
            Some("SHIPFREE".to_string()),
        );
        assert_eq!(quote.subtotal_cents, 4000);
        assert_eq!(quote.discount_cents, 200);
        assert_eq!(quote.shipping_cents, 0);
        assert_eq!(quote.tax_cents, 0);
        assert_eq!(quote.total_cents, 3800);
    }

    // Tier and tax percentages truncate fractional cents, and free shipping
    // is decided from the post-discount merchandise total.
    #[test]
    fn gold_threshold_after_discount() {
        let quote = price_order(
            vec![line("threshold-item", 1, 10_501)],
            CustomerTier::Gold,
            ShippingRegion::Domestic,
            None,
        );
        assert_eq!(quote.subtotal_cents, 10_501);
        assert_eq!(quote.discount_cents, 1050);
        assert_eq!(quote.shipping_cents, 750);
        assert_eq!(quote.tax_cents, 756);
        assert_eq!(quote.total_cents, 10_957);
    }

    // International orders pay the 2500-cent rate unless shipping is waived.
    #[test]
    fn standard_international_paid_shipping() {
        let quote = price_order(
            vec![line("international-item", 2, 1000)],
            CustomerTier::Standard,
            ShippingRegion::International,
            None,
        );
        assert_eq!(quote.subtotal_cents, 2000);
        assert_eq!(quote.discount_cents, 0);
        assert_eq!(quote.shipping_cents, 2500);
        assert_eq!(quote.tax_cents, 0);
        assert_eq!(quote.total_cents, 4500);
    }

    // Arithmetic overflow fails deterministically instead of wrapping.
    #[test]
    fn arithmetic_overflow_fault() {
        let panic = match std::panic::catch_unwind(|| {
            price_order(
                vec![line("overflow-item", 2, i64::MAX)],
                CustomerTier::Standard,
                ShippingRegion::Domestic,
                None,
            )
        }) {
            Ok(_) => panic!("expected order price overflow"),
            Err(panic) => panic,
        };
        let message = panic
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
        assert_eq!(message, Some("order price overflow"));
    }
}
