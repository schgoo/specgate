//! Opt-in field inclusion for `#[derive(SpecEvent)]`.
//!
//! A struct field appears in the spec surface (`to_spec_value` / `$result`)
//! ONLY when tagged `#[spec_event]`. Untagged fields are internal and are
//! excluded. This unifies `to_spec_value` with `emit_fields`, which is already
//! opt-in. Enum variant payloads are intrinsic to the variant and are always
//! included regardless of tagging.

use specgate::{SpecEvent, ToSpecValue, Value};
use std::collections::BTreeMap;

#[derive(SpecEvent)]
struct Mixed {
    #[spec_event]
    visible: i32,
    #[allow(dead_code)]
    internal: i32,
}

#[derive(SpecEvent)]
enum Shape {
    Circle { radius: i32 },
}

#[derive(SpecEvent)]
struct Renamed {
    #[spec_event(name = "spec_key")]
    field_ident: i32,
}

fn as_map(v: Value) -> BTreeMap<String, Value> {
    match v {
        Value::Map(m) => m,
        other => panic!("expected Value::Map, got {other:?}"),
    }
}

#[test]
fn tagged_struct_field_is_in_spec_value() {
    let map = as_map(Mixed { visible: 1, internal: 2 }.to_spec_value());
    assert!(map.contains_key("visible"), "tagged field must appear in to_spec_value");
}

#[test]
fn untagged_struct_field_is_excluded_from_spec_value() {
    let map = as_map(Mixed { visible: 1, internal: 2 }.to_spec_value());
    assert!(
        !map.contains_key("internal"),
        "untagged field must NOT appear in to_spec_value (opt-in model)"
    );
}

#[test]
fn enum_variant_fields_are_always_included() {
    let inner = as_map(
        as_map(Shape::Circle { radius: 5 }.to_spec_value())
            .remove("Circle")
            .expect("Circle variant key present"),
    );
    assert!(inner.contains_key("radius"), "enum variant payload is always included");
}

#[test]
fn spec_event_name_overrides_struct_spec_value_key() {
    let map = as_map(Renamed { field_ident: 7 }.to_spec_value());
    assert!(
        map.contains_key("spec_key"),
        "to_spec_value must key by the #[spec_event(name=...)] override"
    );
    assert!(
        !map.contains_key("field_ident"),
        "to_spec_value must NOT use the raw field ident when a name override is present"
    );
}
