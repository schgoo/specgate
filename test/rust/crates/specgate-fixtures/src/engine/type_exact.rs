// Type-exact matching witnesses: scalar equality must NOT coerce across types.
// `count` is an int, `enabled` a bool, `code` a numeric-looking string. The
// spec asserts each with both the correct type (pass) and the wrong type
// (must fail) to prove the matcher rejects cross-type coercion.
use specgate::*;

#[derive(SpecEvent)]
pub struct Scalars {
    #[spec_event(name = "count")]
    pub count: i32,
    #[spec_event(name = "enabled")]
    pub enabled: bool,
    #[spec_event(name = "code")]
    pub code: String,
}

#[spec_operation("get_scalars", spec = "fixture.type_exact")]
pub fn get_scalars() -> Scalars {
    Scalars {
        count: 5,
        enabled: true,
        code: "7".to_string(),
    }
}
