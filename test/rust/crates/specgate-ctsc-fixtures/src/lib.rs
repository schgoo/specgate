//! CTSC-native behavioral, discovery, and capture fixtures.

specgate::spec_component!("fixture.ctsc");

#[path = "conformance/basic/stateless_add.rs"]
pub mod stateless;

#[path = "registry/focused/rich.rs"]
pub mod rich;

#[path = "registry/focused/setup.rs"]
pub mod setup;

#[path = "registry/focused/faults.rs"]
pub mod faults;

#[path = "registry/focused/fallible_unit.rs"]
pub mod fallible_unit;

pub mod conformance;
pub mod registry;
