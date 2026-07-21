//! Stateful shapes — setup-backed receivers, mutations, multi-step sequences,
//! side-effect setups, and multi-field capture.
pub mod multi_field_capture;
pub mod multi_mutation;
pub mod multi_setup;
pub mod multi_step;
pub mod nested_operations;
pub mod readonly_operation;
pub mod setup_with_params;
pub mod shared_setup;
pub mod side_effect_setup;
pub mod side_effect_setup_with_input;
pub mod simple_output_setup;
pub mod statemachine_counter;
pub mod void_operation;
