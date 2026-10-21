use specgate::spec_operation;

#[spec_operation("lookup_user", spec = "fixture.mock_not_found")]
pub fn lookup_user(id: &str) -> Option<String> {
    (id == "known").then(|| "Known User".to_string())
}

#[spec_operation("get_user", spec = "fixture.mock_not_found")]
pub fn get_user(id: &str) -> Result<String, String> {
    lookup_user(id).ok_or_else(|| format!("user {id} not found"))
}

#[test]
fn missing_fake_response_is_a_declared_error() {
    assert_eq!(
        get_user("missing"),
        Err("user missing not found".to_string())
    );
}
