use specgate::spec_operation;

#[spec_operation("lookup_user", spec = "fixture.mock_field")]
pub fn lookup_user(id: &str) -> Option<String> {
    (id == "42").then(|| "Ada".to_string())
}

#[spec_operation("get_user", spec = "fixture.mock_field")]
pub fn get_user(id: &str) -> Result<String, String> {
    lookup_user(id).ok_or_else(|| format!("user {id} not found"))
}

#[test]
fn deterministic_child_operation_replaces_field_mock() {
    assert_eq!(get_user("42"), Ok("Ada".to_string()));
}
