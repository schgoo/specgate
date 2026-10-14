use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, SpecEvent)]
#[spec_component("fixture.plain_enum")]
// The discriminants deliberately contradict declaration order. CTSC never
// encodes a discriminant, so the published variants must stay Low, Medium,
// High; a producer that ordered by underlying value would reorder them.
pub enum Level {
    Low = 30,
    Medium = 10,
    High = 20,
}

#[spec_operation("level_for", spec = "fixture.plain_enum")]
pub fn level_for(score: i32) -> Level {
    if score < 10 {
        Level::Low
    } else if score < 100 {
        Level::Medium
    } else {
        Level::High
    }
}

#[test]
fn every_payload_free_variant_is_reachable() {
    assert_eq!(level_for(0), Level::Low);
    assert_eq!(level_for(50), Level::Medium);
    assert_eq!(level_for(500), Level::High);
}
