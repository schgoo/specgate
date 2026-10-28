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

pub mod setup_unknown_fill {
    use specgate::{spec_operation, spec_setup};

    #[spec_setup("render", spec = "fixture.setup_unknown_fill", fills = "height")]
    pub fn make_height() -> i32 {
        1
    }

    #[spec_operation("render", spec = "fixture.setup_unknown_fill")]
    pub fn render(width: i32) -> i32 {
        width
    }
}

pub mod setup_ambiguous_fill {
    use specgate::{spec_operation, spec_setup};

    #[spec_setup("render", spec = "fixture.setup_ambiguous_fill")]
    pub fn make_side() -> i32 {
        1
    }

    #[spec_operation("render", spec = "fixture.setup_ambiguous_fill")]
    pub fn render(left: i32, right: i32) -> i32 {
        left + right
    }
}

pub mod setup_duplicate_fill {
    use specgate::{spec_operation, spec_setup};

    #[spec_setup("render", spec = "fixture.setup_duplicate_fill", fills = "width")]
    pub fn make_width() -> i32 {
        1
    }

    #[spec_setup("render", spec = "fixture.setup_duplicate_fill", fills = "width")]
    pub fn make_width_again() -> i32 {
        2
    }

    #[spec_operation("render", spec = "fixture.setup_duplicate_fill")]
    pub fn render(width: i32) -> i32 {
        width
    }
}

pub mod setup_extra_receiver {
    use specgate::{SpecEvent, spec_operation, spec_setup};

    #[derive(Debug, SpecEvent)]
    #[spec_component("fixture.setup_extra_receiver")]
    pub struct Canvas {
        #[spec_event]
        pub width: i32,
    }

    #[spec_setup("render", spec = "fixture.setup_extra_receiver")]
    pub fn make_canvas() -> Canvas {
        Canvas { width: 1 }
    }

    #[spec_setup("render", spec = "fixture.setup_extra_receiver")]
    pub fn make_wider_canvas() -> Canvas {
        Canvas { width: 2 }
    }

    impl Canvas {
        #[spec_operation("render", spec = "fixture.setup_extra_receiver")]
        pub fn render(&self) -> i32 {
            self.width
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

pub mod property_collision {
    use specgate::{SpecEvent, spec_operation};

    #[derive(Debug, SpecEvent)]
    #[spec_component("fixture.property_collision")]
    #[spec_event(setup)]
    pub struct Query {
        pub value: i32,
    }

    #[spec_operation("read", spec = "fixture.property_collision")]
    pub fn read(query: Query, value: i32) -> i32 {
        query.value + value
    }
}

#[cfg(feature = "missing-default")]
mod missing_default {
    include!("templates/missing_default.rs.in");
}

#[cfg(feature = "compile-error")]
mod compile_error {
    include!("templates/compile_error.rs.in");
}
