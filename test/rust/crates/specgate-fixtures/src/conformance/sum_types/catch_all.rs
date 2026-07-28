// Catch-all fallible op. The C# side uses `[SpecException]` with no declared
// types, so EVERY thrown exception maps to the Err arm regardless of type — the
// two error cases throw DIFFERENT exception types to prove the catch-all is not
// type-filtered (no `$fault` path). The Rust target returns `Err` directly.
use specgate::*;

#[spec_operation("require_in_range", spec = "fixture.catch_all")]
pub fn require_in_range(x: i32) -> Result<i32, String> {
    if x < 0 {
        return Err("too small".to_string());
    }
    if x > 100 {
        return Err("too big".to_string());
    }
    Ok(x)
}
