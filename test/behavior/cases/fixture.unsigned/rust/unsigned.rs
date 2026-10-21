use specgate::spec_operation;

#[spec_operation("echo_u32", spec = "fixture.unsigned")]
pub fn echo_u32(value: u32) -> u32 {
    value
}

#[spec_operation("echo_u64", spec = "fixture.unsigned")]
pub fn echo_u64(value: u64) -> u64 {
    value
}

#[test]
fn unsigned_boundaries_round_trip() {
    for value in [0, 2_147_483_647, 2_147_483_648, u32::MAX] {
        assert_eq!(echo_u32(value), value);
    }
    for value in [
        0,
        9_223_372_036_854_775_807,
        9_223_372_036_854_775_808,
        u64::MAX,
    ] {
        assert_eq!(echo_u64(value), value);
    }
}
