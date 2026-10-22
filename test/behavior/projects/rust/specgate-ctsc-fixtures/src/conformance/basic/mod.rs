//! Basic scalar, observation, naming, and source-layout behaviors.

#[path = "../../../../../../cases/fixture.checkpoint_inline/rust/checkpoint_inline.rs"]
pub mod checkpoint_inline;
#[path = "../../../../../../cases/fixture.default_input/rust/default_input.rs"]
pub mod default_input;
#[path = "../../../../../../cases/fixture.keyword_collision/rust/keyword_collision.rs"]
pub mod keyword_collision;
#[path = "../../../../../../cases/fixture.multi_case/rust/multi_case.rs"]
pub mod multi_case;
#[path = "../../../../../../cases/fixture.multi_toplevel/rust/multi_toplevel_a.rs"]
pub mod multi_toplevel_a;
#[path = "../../../../../../cases/fixture.multi_toplevel/rust/multi_toplevel_b.rs"]
pub mod multi_toplevel_b;
#[path = "../../../../../../cases/fixture.named_inputs/rust/named_inputs.rs"]
pub mod named_inputs;
#[path = "../../../../../../cases/fixture.nested_path/rust/nested_path.rs"]
pub mod nested_path;
#[path = "../../../../../../cases/fixture.property_add/rust/property_add.rs"]
pub mod property_add;

pub use crate::stateless as stateless_add;
