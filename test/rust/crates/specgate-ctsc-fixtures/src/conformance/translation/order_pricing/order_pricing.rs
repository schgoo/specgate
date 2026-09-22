use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.order_pricing_translation")]
pub struct OrderLine {
    #[spec_event]
    pub sku: String,
    #[spec_event]
    pub quantity: i32,
    #[spec_event]
    pub unit_price_cents: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.order_pricing_translation")]
pub enum CustomerTier {
    Standard,
    Silver,
    Gold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.order_pricing_translation")]
pub enum ShippingRegion {
    Domestic,
    International,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.order_pricing_translation")]
pub struct PriceQuote {
    #[spec_event]
    pub subtotal_cents: i64,
    #[spec_event]
    pub discount_cents: i64,
    #[spec_event]
    pub shipping_cents: i64,
    #[spec_event]
    pub tax_cents: i64,
    #[spec_event]
    pub total_cents: i64,
}

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

#[spec_operation("price_order", spec = "fixture.order_pricing_translation")]
pub fn price_order(
    lines: Vec<OrderLine>,
    customer_tier: CustomerTier,
    shipping_region: ShippingRegion,
    coupon: Option<String>,
) -> PriceQuote {
    let mut subtotal_cents = 0_i64;
    let mut bulk_discount_cents = 0_i64;
    for line in &lines {
        let line_total = checked(i64::from(line.quantity).checked_mul(line.unit_price_cents));
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
    let tier_discount_percent = match customer_tier {
        CustomerTier::Standard => 0,
        CustomerTier::Silver => SILVER_DISCOUNT_PERCENT,
        CustomerTier::Gold => GOLD_DISCOUNT_PERCENT,
    };
    let tier_discount_cents = checked(
        merchandise_cents
            .checked_mul(tier_discount_percent)
            .and_then(|value| value.checked_div(100)),
    );
    merchandise_cents = checked(merchandise_cents.checked_sub(tier_discount_cents));

    let coupon_discount_cents = match coupon.as_deref() {
        Some("SAVE500") => COUPON_SAVE_CAP.min(merchandise_cents),
        _ => 0,
    };
    merchandise_cents = checked(merchandise_cents.checked_sub(coupon_discount_cents));

    let discount_cents = checked(
        bulk_discount_cents
            .checked_add(tier_discount_cents)
            .and_then(|value| value.checked_add(coupon_discount_cents)),
    );
    let shipping_cents = if coupon.as_deref() == Some("SHIPFREE")
        || merchandise_cents >= FREE_SHIPPING_THRESHOLD_CENTS
    {
        0
    } else {
        match shipping_region {
            ShippingRegion::Domestic => DOMESTIC_SHIPPING_CENTS,
            ShippingRegion::International => INTERNATIONAL_SHIPPING_CENTS,
        }
    };
    let tax_cents = match shipping_region {
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

    #[test]
    fn standard_domestic_order() {
        assert_eq!(
            price_order(
                vec![line("widget", 2, 2500), line("cable", 3, 500)],
                CustomerTier::Standard,
                ShippingRegion::Domestic,
                None,
            ),
            PriceQuote {
                subtotal_cents: 6500,
                discount_cents: 0,
                shipping_cents: 750,
                tax_cents: 520,
                total_cents: 7770,
            }
        );
    }

    #[test]
    fn gold_bulk_order_with_coupon() {
        assert_eq!(
            price_order(
                vec![line("bulk-pack", 10, 1000), line("premium-part", 2, 2500)],
                CustomerTier::Gold,
                ShippingRegion::Domestic,
                Some("SAVE500".to_string()),
            ),
            PriceQuote {
                subtotal_cents: 15_000,
                discount_cents: 2900,
                shipping_cents: 0,
                tax_cents: 968,
                total_cents: 13_068,
            }
        );
    }

    #[test]
    fn silver_international_shipfree() {
        assert_eq!(
            price_order(
                vec![line("export-item", 4, 1000)],
                CustomerTier::Silver,
                ShippingRegion::International,
                Some("SHIPFREE".to_string()),
            ),
            PriceQuote {
                subtotal_cents: 4000,
                discount_cents: 200,
                shipping_cents: 0,
                tax_cents: 0,
                total_cents: 3800,
            }
        );
    }

    #[test]
    fn gold_threshold_after_discount() {
        assert_eq!(
            price_order(
                vec![line("threshold-item", 1, 10_501)],
                CustomerTier::Gold,
                ShippingRegion::Domestic,
                None,
            ),
            PriceQuote {
                subtotal_cents: 10_501,
                discount_cents: 1050,
                shipping_cents: 750,
                tax_cents: 756,
                total_cents: 10_957,
            }
        );
    }

    #[test]
    fn standard_international_paid_shipping() {
        assert_eq!(
            price_order(
                vec![line("international-item", 2, 1000)],
                CustomerTier::Standard,
                ShippingRegion::International,
                None,
            ),
            PriceQuote {
                subtotal_cents: 2000,
                discount_cents: 0,
                shipping_cents: 2500,
                tax_cents: 0,
                total_cents: 4500,
            }
        );
    }

    #[test]
    fn arithmetic_overflow_fault() {
        let panic = std::panic::catch_unwind(|| {
            price_order(
                vec![line("overflow-item", 2, i64::MAX)],
                CustomerTier::Standard,
                ShippingRegion::Domestic,
                None,
            )
        });
        assert!(panic.is_err());
    }
}
