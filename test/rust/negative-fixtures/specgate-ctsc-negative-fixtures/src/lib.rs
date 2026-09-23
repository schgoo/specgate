//! Isolated sources for deterministic negative CTSC discovery cases.
//!
//! Every module here is a case that discovery, registry encoding, or the Rust
//! compiler must reject. Only `compile_error` is feature-gated, because a
//! deliberate syntax error cannot be part of a building crate.

specgate::spec_component!("fixture.ctsc_negative");

pub mod duplicate_identity {
    use specgate::spec_operation;

    #[spec_operation("render", spec = "fixture.duplicate_identity")]
    pub fn render_one() -> String {
        "one".to_string()
    }

    #[spec_operation("render", spec = "fixture.duplicate_identity")]
    pub fn render_two() -> String {
        "two".to_string()
    }
}

pub mod missing_operation {
    use specgate::{SpecEvent, spec_setup};

    #[derive(Debug, SpecEvent)]
    #[spec_component("fixture.missing_operation")]
    pub struct Counter {
        #[spec_event]
        pub count: i32,
    }

    #[spec_setup("increment", spec = "fixture.missing_operation")]
    pub fn make_counter() -> Counter {
        Counter { count: 0 }
    }

    impl Counter {
        pub fn increment(&mut self) {
            self.count += 1;
        }
    }
}

pub mod missing_setup {
    use specgate::{SpecEvent, spec_operation};

    #[derive(Debug, SpecEvent)]
    #[spec_component("fixture.missing_setup")]
    pub struct Counter {
        #[spec_event]
        pub count: i32,
    }

    impl Counter {
        #[spec_operation("increment", spec = "fixture.missing_setup")]
        pub fn increment(&mut self) {
            self.count += 1;
        }
    }
}

pub mod private_operation {
    use specgate::spec_operation;

    #[spec_operation("secret", spec = "fixture.private_operation")]
    fn secret() -> i32 {
        42
    }
}

pub mod unresolved_type;
pub mod value_surfaces;

#[cfg(feature = "compile-error")]
mod compile_error {
    include!("templates/compile_error.rs.in");
}
