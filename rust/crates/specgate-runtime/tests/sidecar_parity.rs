//! Byte-level contract for the native capture sidecar.
//!
//! Rust and C# both write the same sidecar document, and the CLI reads either
//! one without knowing which producer wrote it. That only holds if the two
//! producers agree on field order, field naming, optional-field omission, and
//! value encoding — none of which a shared schema would pin down.
//!
//! So the agreement is written down once, as bytes, in
//! `test/goldens/capture-sidecar/parity.json`. This test asserts Rust produces
//! them; `CaptureJsonParityTests` in `SpecGate.Runtime.Tests` asserts C# does
//! too. Rust is the reference: when the protocol changes, this test fails
//! first, its panic prints the new bytes, and the file is updated from them.

use specgate_runtime::capture::{Capture, Completion, Observation, OperationSpan, SpanBoundary, SpanDeps, SpanId, Status, TraceId};
use specgate_runtime::value::Value;
use specgate_runtime::{ComponentId, OperationName};
use std::collections::{BTreeMap, BTreeSet};

/// The checked-in bytes both producers must emit.
const GOLDEN: &str = include_str!("../../../../test/goldens/capture-sidecar/parity.json");

#[test]
fn the_sidecar_document_matches_the_cross_language_golden() {
    let actual = serde_json::to_string(&parity_capture()).expect("the parity capture serializes");
    assert_eq!(
        actual,
        GOLDEN.trim_end_matches('\n'),
        "native sidecar bytes changed; update test/goldens/capture-sidecar/parity.json to:\n{actual}"
    );
}

/// A capture that reaches every completion channel, every value kind, and both
/// optional-field states, so the golden pins the whole wire surface.
fn parity_capture() -> Capture {
    Capture {
        trace_id: id::trace("0af7651916cd43dd8448eb211c80319c"),
        scenario_name: "parity".to_owned(),
        run: SpanBoundary {
            span_id: id::span("aaaaaaaaaaaa0001"),
            parent_id: None,
            start_ns: 0,
            end_ns: 100,
            status: Status::Error,
        },
        scenario: SpanBoundary {
            span_id: id::span("aaaaaaaaaaaa0002"),
            parent_id: Some(id::span("aaaaaaaaaaaa0001")),
            start_ns: 1,
            end_ns: 99,
            status: Status::Error,
        },
        operations: vec![
            every_value_kind(),
            declared_error_with_payload(),
            declared_error_without_payload(),
            unmapped_fault(),
            explicit_empty(),
            abandoned(),
            unit_without_completion(),
            text_and_numbers(),
        ],
    }
}

/// Strings and floats whose `serde_json` spelling is easy to get wrong: control
/// and line-separator characters, a non-BMP character, float notation
/// boundaries, and keys whose UTF-8 order differs from their UTF-16 order.
fn text_and_numbers() -> OperationSpan {
    let floats = [1e-5, 1e-6, 1e15, 1e16, 1.5e300, 5e-324, -0.0, -2.5, 123_456_789.125, 0.1];
    let inputs = BTreeMap::from([
        ("\u{ffff}".to_owned(), Value::Bool(true)),
        ("\u{1f600}".to_owned(), Value::Bool(false)),
        ("floats".to_owned(), Value::List(floats.into_iter().map(Value::Float).collect())),
        (
            "text".to_owned(),
            Value::String("\u{2028}\u{1f600}\u{1f}\u{8}\u{c}\t\u{7f}\u{e9}/".to_owned()),
        ),
    ]);

    OperationSpan::builder(deps(15, "aaaaaaaaaaaa0017", "aaaaaaaaaaaa0002", "text_and_numbers", 28))
        .end_time(30)
        .inputs(inputs)
        .completion(Completion::Empty { order: 16, time_ns: 29 })
        .build()
        .expect("the text span is well formed")
}

/// A result completion whose inputs reach every semantic value kind, plus one
/// observation, so the golden pins the value encoding exhaustively.
fn every_value_kind() -> OperationSpan {
    let inputs = BTreeMap::from([
        ("a_string".to_owned(), Value::String("hello \"world\"\n".to_owned())),
        ("b_integer".to_owned(), Value::Integer(i64::MIN)),
        ("c_unsigned".to_owned(), Value::Unsigned(u64::MAX)),
        ("d_float".to_owned(), Value::Float(1.5)),
        ("e_bool".to_owned(), Value::Bool(true)),
        ("f_list".to_owned(), Value::List(vec![Value::Integer(1), Value::Integer(2)])),
        (
            "g_map".to_owned(),
            Value::Map(BTreeMap::from([
                ("inner".to_owned(), Value::Bool(false)),
                ("outer".to_owned(), Value::String("x".to_owned())),
            ])),
        ),
        (
            "h_set".to_owned(),
            Value::Set(BTreeSet::from([Value::String("a".to_owned()), Value::String("b".to_owned())])),
        ),
        ("i_unit".to_owned(), Value::Map(BTreeMap::new())),
    ]);

    OperationSpan::builder(deps(0, "aaaaaaaaaaaa0010", "aaaaaaaaaaaa0002", "every_value_kind", 2))
        .end_time(5)
        .inputs(inputs)
        .observations(vec![Observation {
            order: 1,
            time_ns: 3,
            name: "progress".to_owned(),
            value: Value::Unsigned(1),
        }])
        .completion(Completion::Result {
            order: 2,
            time_ns: 4,
            value: Value::Integer(7),
        })
        .build()
        .expect("the value-kind span is well formed")
}

/// A declared error that carries a payload.
fn declared_error_with_payload() -> OperationSpan {
    OperationSpan::builder(deps(3, "aaaaaaaaaaaa0011", "aaaaaaaaaaaa0010", "error_with_payload", 6))
        .end_time(9)
        .status(Status::Error)
        .completion(Completion::Error {
            order: 4,
            time_ns: 8,
            name: "error".to_owned(),
            value: Some(Value::String("boom".to_owned())),
        })
        .build()
        .expect("the payload error span is well formed")
}

/// A declared error whose value channel is empty, pinning the one optional
/// field the protocol omits rather than writes as null.
fn declared_error_without_payload() -> OperationSpan {
    OperationSpan::builder(deps(5, "aaaaaaaaaaaa0012", "aaaaaaaaaaaa0002", "error_without_payload", 10))
        .end_time(13)
        .status(Status::Error)
        .completion(Completion::Error {
            order: 6,
            time_ns: 12,
            name: "error".to_owned(),
            value: None,
        })
        .build()
        .expect("the bare error span is well formed")
}

/// An unmapped target failure.
fn unmapped_fault() -> OperationSpan {
    OperationSpan::builder(deps(7, "aaaaaaaaaaaa0013", "aaaaaaaaaaaa0002", "unmapped_fault", 14))
        .end_time(17)
        .status(Status::Error)
        .completion(Completion::Fault {
            order: 8,
            time_ns: 16,
            fault_type: "specgate.unexpected_target_fault".to_owned(),
            message: "operation unwound before returning".to_owned(),
            observer: "target".to_owned(),
        })
        .build()
        .expect("the fault span is well formed")
}

/// A successful absence from an operation that has a value channel.
fn explicit_empty() -> OperationSpan {
    OperationSpan::builder(deps(9, "aaaaaaaaaaaa0014", "aaaaaaaaaaaa0002", "explicit_empty", 18))
        .end_time(21)
        .completion(Completion::Empty { order: 10, time_ns: 20 })
        .build()
        .expect("the empty span is well formed")
}

/// An operation that stopped without reaching any outcome.
fn abandoned() -> OperationSpan {
    OperationSpan::builder(deps(11, "aaaaaaaaaaaa0015", "aaaaaaaaaaaa0002", "abandoned", 22))
        .end_time(25)
        .status(Status::Unset)
        .completion(Completion::Abandoned { order: 12, time_ns: 24 })
        .build()
        .expect("the abandoned span is well formed")
}

/// A unit operation. It has no value channel, so it reports no completion at
/// all; the builder refuses that, which is why this span is assembled directly.
fn unit_without_completion() -> OperationSpan {
    let mut span = OperationSpan::builder(deps(13, "aaaaaaaaaaaa0016", "aaaaaaaaaaaa0002", "unit", 26))
        .end_time(27)
        .completion(Completion::Empty { order: 14, time_ns: 27 })
        .build()
        .expect("the unit span is well formed");
    span.completion = None;
    span
}

fn deps(order: u64, span_id: &str, parent_id: &str, operation: &str, start_ns: i64) -> SpanDeps {
    SpanDeps {
        order,
        span_id: id::span(span_id),
        parent_id: id::span(parent_id),
        component_id: ComponentId::from("parity.component"),
        operation_name: OperationName::from(operation),
        start_ns,
    }
}

mod id {
    use super::{SpanId, TraceId};

    pub fn trace(value: &str) -> TraceId {
        TraceId::parse(value).expect("the fixture trace ID is valid")
    }

    pub fn span(value: &str) -> SpanId {
        SpanId::parse(value).expect("the fixture span ID is valid")
    }
}
