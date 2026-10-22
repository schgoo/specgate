//! CTSC-native behavioral, discovery, and capture fixtures.

specgate::spec_component!("fixture.ctsc");

#[path = "../../../../cases/fixture.stateless_add/rust/stateless_add.rs"]
pub mod stateless;

#[path = "../../../../cases/fixture.unsigned/rust/unsigned.rs"]
pub mod unsigned;

#[path = "../../../../cases/fixture.rich/rust/rich.rs"]
pub mod rich;

#[path = "../../../../cases/fixture.setup/rust/setup.rs"]
pub mod setup;

#[path = "../../../../cases/fixture.faults/rust/faults.rs"]
pub mod faults;

#[path = "../../../../cases/fixture.fallible_unit/rust/fallible_unit.rs"]
pub mod fallible_unit;

#[path = "conformance/mod.rs"]
pub mod conformance;
#[path = "registry/mod.rs"]
pub mod registry;
