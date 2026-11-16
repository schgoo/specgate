use specgate::spec_operation;

pub struct Context(u8);

impl Default for Context {
    fn default() -> Self {
        Self(17)
    }
}

#[spec_operation("add", spec = "fixture.stateless_add")]
pub fn add(
    #[spec_input("a", ignore)] first: Context,
    a: i32,
    #[spec_input(ignore)] middle: Context,
    b: i32,
) -> i32 {
    assert_eq!((first.0, middle.0), (17, 17));
    a + b
}

#[test]
fn add_two_and_three() {
    assert_eq!(add(Context(17), 2, Context(17), 3), 5);
}
