use specgate::spec_operation;

pub mod field_lookup {
    use super::*;

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
}

pub mod multiple_responses {
    use super::*;

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
}

pub mod not_found {
    use super::*;

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
}
