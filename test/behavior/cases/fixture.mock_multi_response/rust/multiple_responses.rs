use specgate::spec_operation;

#[spec_operation("lookup_user", spec = "fixture.mock_multi_response")]
pub fn lookup_user(id: &str) -> Option<String> {
    match id {
        "1" => Some("Ada".to_string()),
        "2" => Some("Grace".to_string()),
        _ => None,
    }
}

#[spec_operation("get_users", spec = "fixture.mock_multi_response")]
pub fn get_two_users(id_a: &str, id_b: &str) -> Result<String, String> {
    let a = lookup_user(id_a).ok_or_else(|| format!("user {id_a} not found"))?;
    let b = lookup_user(id_b).ok_or_else(|| format!("user {id_b} not found"))?;
    Ok(format!("{a} and {b}"))
}

#[test]
fn deterministic_child_operation_returns_input_specific_values() {
    assert_eq!(get_two_users("1", "2"), Ok("Ada and Grace".to_string()));
}
