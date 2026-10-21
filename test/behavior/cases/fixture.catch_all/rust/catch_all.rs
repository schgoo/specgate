use specgate::spec_operation;

#[spec_operation("require_in_range", spec = "fixture.catch_all")]
pub fn require_in_range(x: i32) -> Result<i32, String> {
    if x < 0 {
        Err("too small".to_string())
    } else if x > 100 {
        Err("too big".to_string())
    } else {
        Ok(x)
    }
}

#[test]
fn catch_all_error_surface_has_success_and_distinct_failures() {
    assert_eq!(require_in_range(40), Ok(40));
    assert_eq!(require_in_range(-1), Err("too small".to_string()));
    assert_eq!(require_in_range(101), Err("too big".to_string()));
}
