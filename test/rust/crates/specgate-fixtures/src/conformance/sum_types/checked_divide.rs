// Fallible op with a declared error arm AND an undeclared panic path. Exercises
// precise `[SpecException]` resolution on the C# side: the declared exception
// maps to the Err arm, while an undeclared throw falls through to `$fault`. The
// Rust target realizes the same contract with `Result` plus `panic!`.
use specgate::*;

#[spec_operation("checked_divide", spec = "fixture.checked_divide")]
pub fn checked_divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        return Err("division by zero".to_string());
    }
    if b < 0 {
        panic!("negative divisor");
    }
    Ok(a / b)
}
