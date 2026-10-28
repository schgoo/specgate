specgate::spec_component!("fixture.binary_unit");

fn main() {}

#[cfg(test)]
mod tests {
    #[specgate::spec_operation("increment", spec = "fixture.binary_unit")]
    pub fn increment(value: i32) -> i32 {
        value + 1
    }

    #[test]
    fn binary_unit_operation() {
        assert_eq!(increment(10), 11);
    }
}
