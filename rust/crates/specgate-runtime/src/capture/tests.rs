use super::io::persistence_stage::PersistenceStage;
use super::*;

fn native_config<'a>(span_ids: impl AsRef<[&'a str]>) -> Config {
    let span_ids = span_ids.as_ref();
    Config::builder(ConfigDeps {
        scenario_name: "scenario".to_string(),
        trace_id: "11111111111111111111111111111111".try_into().unwrap(),
        run_id: "1111111111111101".try_into().unwrap(),
        scenario_id: "1111111111111102".try_into().unwrap(),
    })
    .operation_ids(span_ids.iter().map(|id| (*id).try_into().unwrap()).collect::<Vec<_>>())
    .start_time(1_000)
    .clock_step(10)
    .build()
    .unwrap()
}

#[derive(Clone, Copy)]
struct SetupTarget {
    component_id: registry::ComponentName,
    operation_name: registry::OpName,
}
impl SetupTarget {
    const fn new(component_id: &'static str, operation_name: &'static str) -> Self {
        Self {
            component_id: registry::ComponentName::new(component_id),
            operation_name: registry::OpName::new(operation_name),
        }
    }
}
#[derive(Clone, Copy)]
struct SetupDeclaration {
    module_path: registry::ModulePath,
    fn_name: registry::FnName,
    fills: Option<registry::FieldName>,
}
impl SetupDeclaration {
    const fn new(module_path: &'static str, fn_name: &'static str, fills: Option<&'static str>) -> Self {
        Self {
            module_path: registry::ModulePath::new(module_path),
            fn_name: registry::FnName::new(fn_name),
            fills: match fills {
                Some(fills) => Some(registry::FieldName::new(fills)),
                None => None,
            },
        }
    }
}
fn setup(target: SetupTarget, declaration: SetupDeclaration) -> SetupProvenance {
    SetupProvenance {
        component_id: target.component_id,
        operation_name: target.operation_name,
        module_path: declaration.module_path,
        fn_name: declaration.fn_name,
        fills: declaration.fills,
    }
}
fn start_sidecar(configuration: FakeFs) -> Arc<Mutex<FakeFs>> {
    let (file_system, state) = FileSystem::fake(configuration);
    start_with(native_config([]), Some(PathBuf::from("fake-sidecar.json")), file_system).unwrap();
    state
}

#[test]
fn active_setup() {
    assert!(recorded_inputs("fixture.native", "increment").unwrap().is_empty());
    record_setup(
        setup(
            SetupTarget::new("fixture.native", "increment"),
            SetupDeclaration::new("fixture", "make", None),
        ),
        || vec![(registry::FieldName::new("initial"), Value::Integer(4))],
    )
    .unwrap();
    assert!(
        recorded_inputs("fixture.native", "increment").unwrap().is_empty(),
        "an inactive, unrequested capture records nothing"
    );

    start(native_config([])).unwrap();
    record_setup(
        setup(
            SetupTarget::new("fixture.native", "increment"),
            SetupDeclaration::new("fixture", "make", None),
        ),
        || vec![(registry::FieldName::new("initial"), Value::Integer(4))],
    )
    .unwrap();
    let scope = begin_operation(ComponentId::from("fixture.native"), OperationName::from("increment")).unwrap();
    scope.unit().unwrap();

    let capture = finish().unwrap();
    assert_eq!(
        capture.operations[0].inputs,
        BTreeMap::from([("initial".to_string(), Value::Integer(4))])
    );
    assert!(
        recorded_inputs("fixture.native", "increment").unwrap().is_empty(),
        "finishing a session clears recorded setup inputs"
    );
}

#[test]
fn metadata_collision() {
    start(native_config([])).unwrap();
    record_setup(
        setup(
            SetupTarget::new("fixture.native", "combine"),
            SetupDeclaration::new("fixture", "make", Some("left")),
        ),
        || vec![(registry::FieldName::new("seed"), Value::Integer(1))],
    )
    .unwrap();
    record_setup(
        setup(
            SetupTarget::new("fixture.native", "combine"),
            SetupDeclaration::new("fixture", "make", Some("right")),
        ),
        || vec![(registry::FieldName::new("seed"), Value::Integer(2))],
    )
    .unwrap();
    let panic = std::panic::catch_unwind(|| recorded_inputs("fixture.native", "combine"));
    setup::clear();
    finish().unwrap();
    assert!(
        panic
            .unwrap_err()
            .downcast_ref::<String>()
            .is_some_and(|message| message.contains("capture registry invariant violated"))
    );
}

#[test]
fn input_collision() {
    start(native_config([])).unwrap();
    let panic = std::panic::catch_unwind(|| {
        record_setup(
            SetupProvenance {
                component_id: registry::ComponentName::new("fixture.native"),
                operation_name: registry::OpName::new("combine"),
                module_path: registry::ModulePath::new("fixture"),
                fn_name: registry::FnName::new("make"),
                fills: Some(registry::FieldName::new("left")),
            },
            || {
                vec![
                    (registry::FieldName::new("seed"), Value::Integer(1)),
                    (registry::FieldName::new("seed"), Value::Integer(2)),
                ]
            },
        )
    });
    finish().unwrap();
    assert!(
        panic
            .unwrap_err()
            .downcast_ref::<String>()
            .is_some_and(|message| message.contains("generated setup invariant violated"))
    );
}

#[test]
fn identical_setup() {
    start(native_config([])).unwrap();
    for _run in 0..2 {
        record_setup(
            setup(
                SetupTarget::new("fixture.native", "increment"),
                SetupDeclaration::new("fixture", "make", None),
            ),
            || vec![(registry::FieldName::new("initial"), Value::Integer(4))],
        )
        .unwrap();
    }
    assert_eq!(
        recorded_inputs("fixture.native", "increment").unwrap(),
        BTreeMap::from([("initial".to_string(), Value::Integer(4))]),
        "a repeat construction with the same inputs is unambiguous"
    );
    finish().unwrap();
}

#[test]
fn different_setup() {
    start(native_config([])).unwrap();
    record_setup(
        setup(
            SetupTarget::new("fixture.native", "increment"),
            SetupDeclaration::new("fixture", "make", None),
        ),
        || vec![(registry::FieldName::new("initial"), Value::Integer(4))],
    )
    .unwrap();
    let error = record_setup(
        setup(
            SetupTarget::new("fixture.native", "increment"),
            SetupDeclaration::new("fixture", "make", None),
        ),
        || vec![(registry::FieldName::new("initial"), Value::Integer(9))],
    )
    .unwrap_err();
    assert_eq!(error.kind(), CaptureErrorKind::SetupAmbiguity);
    assert!(error.to_string().contains("ran twice in one capture with different inputs"));
    assert_eq!(
        recorded_inputs("fixture.native", "increment").unwrap(),
        BTreeMap::from([("initial".to_string(), Value::Integer(4))]),
        "a rejected repeat leaves the first construction untouched"
    );
    finish().unwrap();
}

#[test]
fn exact_identity() {
    assert!(!inputs_match(
        &BTreeMap::from([("seed".to_string(), Value::Integer(1))]),
        &BTreeMap::from([("seed".to_string(), Value::Float(1.0))])
    ));
    assert!(!inputs_match(
        &BTreeMap::from([("seed".to_string(), Value::List(vec![Value::Integer(1)]))]),
        &BTreeMap::from([("seed".to_string(), Value::Set(BTreeSet::from([Value::Integer(1)])))])
    ));
    assert!(!inputs_match(
        &BTreeMap::from([("seed".to_string(), Value::Integer(1))]),
        &BTreeMap::new()
    ));
    assert!(inputs_match(
        &BTreeMap::from([(
            "seed".to_string(),
            Value::Map(BTreeMap::from([("a".to_string(), Value::Bool(true))]))
        )]),
        &BTreeMap::from([(
            "seed".to_string(),
            Value::Map(BTreeMap::from([("a".to_string(), Value::Bool(true))]))
        )])
    ));
}

#[test]
fn clears_setup() {
    start(native_config([])).unwrap();
    record_setup(
        setup(
            SetupTarget::new("fixture.native", "increment"),
            SetupDeclaration::new("fixture", "make", None),
        ),
        || vec![(registry::FieldName::new("initial"), Value::Integer(4))],
    )
    .unwrap();
    finish().unwrap();

    start(native_config([])).unwrap();
    assert!(
        recorded_inputs("fixture.native", "increment").unwrap().is_empty(),
        "a new session must not inherit the previous session's construction inputs"
    );
    record_setup(
        setup(
            SetupTarget::new("fixture.native", "increment"),
            SetupDeclaration::new("fixture", "make", None),
        ),
        || vec![(registry::FieldName::new("initial"), Value::Integer(9))],
    )
    .unwrap();
    assert_eq!(
        recorded_inputs("fixture.native", "increment").unwrap(),
        BTreeMap::from([("initial".to_string(), Value::Integer(9))]),
        "stale state would have made this differing repeat ambiguous"
    );
    finish().unwrap();
}

#[test]
fn inactive_scope() {
    let mut scope = begin_operation(ComponentId::from("fixture"), OperationName::from("noop")).unwrap();
    scope.input("value", Value::Integer(2)).unwrap();
    scope.result(Value::Integer(4)).unwrap();
}

#[test]
fn sidecar_failures() {
    for stage in [
        PersistenceStage::CreateDirectory,
        PersistenceStage::Create,
        PersistenceStage::Write,
        PersistenceStage::Flush,
        PersistenceStage::Sync,
        PersistenceStage::Replace,
    ] {
        let file_system = start_sidecar(FakeFs {
            fail_stage: Some(stage),
            ..FakeFs::default()
        });
        let scope = begin_operation(ComponentId::from("fixture.native"), OperationName::from("persist")).unwrap();
        let error = scope.unit().expect_err("the injected persistence stage must fail");
        assert!(error.to_string().contains(generated::FAILURE_MARKER), "{error}");
        assert!(error.to_string().contains(&format!("{stage:?}")), "{error}");
        assert!(
            begin_operation(ComponentId::from("fixture.native"), OperationName::from("later")).is_err(),
            "a persistence failure must terminalize the active session"
        );
        assert!(finish().is_err(), "finalization must preserve the terminal persistence failure");
        assert!(!file_system.lock().expect("fake filesystem mutex poisoned").calls.is_empty());
    }
}

#[test]
fn replacement_retry() {
    let file_system = start_sidecar(FakeFs {
        replace_failures: 2,
        ..FakeFs::default()
    });
    let scope = begin_operation(ComponentId::from("fixture.native"), OperationName::from("retry")).unwrap();
    scope.unit().unwrap();
    finish().unwrap();

    let file_system = file_system.lock().expect("fake filesystem mutex poisoned");
    assert_eq!(
        file_system
            .calls
            .iter()
            .filter(|stage| **stage == PersistenceStage::Replace)
            .count(),
        4
    );
    assert_eq!(file_system.snapshots.len(), 2);
}

#[test]
fn cumulative_sidecars() {
    let file_system = start_sidecar(FakeFs::default());
    let first = begin_operation(ComponentId::from("fixture.native"), OperationName::from("first")).unwrap();
    first.unit().unwrap();
    assert_eq!(file_system.lock().expect("fake filesystem mutex poisoned").snapshots.len(), 1);
    let second = begin_operation(ComponentId::from("fixture.native"), OperationName::from("second")).unwrap();
    second.result(Value::Integer(2)).unwrap();
    assert_eq!(file_system.lock().expect("fake filesystem mutex poisoned").snapshots.len(), 2);
    finish().unwrap();

    let file_system = file_system.lock().expect("fake filesystem mutex poisoned");
    let snapshots = &file_system.snapshots;
    assert_eq!(snapshots.len(), 3);
    let first: Capture = serde_json::from_slice(&snapshots[0]).unwrap();
    assert_eq!(first.operations.len(), 1);
    assert_eq!(first.operations[0].operation_name, "first");
    let capture: Capture = serde_json::from_slice(&snapshots[2]).unwrap();
    assert_eq!(capture.operations.len(), 2);
    assert_eq!(capture.operations[0].operation_name, "first");
    assert_eq!(capture.operations[1].operation_name, "second");
}

#[test]
fn deferred_fault() {
    let file_system = start_sidecar(FakeFs::default());
    let panic = std::panic::catch_unwind(|| {
        let _scope = begin_operation(ComponentId::from("fixture.native"), OperationName::from("fault")).unwrap();
        panic!("target panic");
    });
    assert!(panic.is_err());
    assert!(
        file_system.lock().expect("fake filesystem mutex poisoned").calls.is_empty(),
        "unwind cleanup must not perform persistence I/O"
    );

    let capture = finish().unwrap();
    assert!(matches!(capture.operations[0].completion, Some(Completion::Fault { .. })));
    assert_eq!(file_system.lock().expect("fake filesystem mutex poisoned").snapshots.len(), 1);
}

#[test]
fn instrumented_future_is_send_when_inner_future_is_send() {
    fn assert_send<T: Send>(_: &T) {}

    let future = instrument_async_operation(capture_async_context(), std::future::ready(()));

    assert_send(&future);
}

#[test]
fn unfinished_panics() {
    let file_system = start_sidecar(FakeFs::default());
    let outcome = std::panic::catch_unwind(|| {
        let scope = begin_operation(ComponentId::from("fixture.native"), OperationName::from("unfinished")).unwrap();
        drop(scope);
    });
    assert!(outcome.is_ok(), "scope destruction must remain infallible");
    assert!(file_system.lock().expect("fake filesystem mutex poisoned").calls.is_empty());
    let error = finish().expect_err("unfinished scope cleanup must terminalize the session");
    assert!(error.to_string().contains("closed without completion"), "{error}");
}

// A regression here aborts the process ("fatal runtime error: thread local
// panicked on drop") and takes the whole specgate-runtime test binary with it
// rather than reporting a failure against this test.
#[test]
fn abandonment_during_thread_local_teardown_does_not_abort() {
    struct AbandonOnTeardown(Option<OperationScope>);
    impl Drop for AbandonOnTeardown {
        fn drop(&mut self) {
            drop(self.0.take());
        }
    }
    thread_local! {
        static TEARDOWN: RefCell<Option<AbandonOnTeardown>> = const { RefCell::new(None) };
    }

    let worker = std::thread::spawn(move || {
        // Touch the probe thread local *before* the capture session so its
        // destructor is registered first. Destruction order is unspecified, but
        // where it is reverse-registration this makes the probe run after
        // `SESSION` is already destroyed, which is what `current_collector`
        // reads with `try_with` rather than `with`.
        TEARDOWN.with(|slot| assert!(slot.borrow().is_none()));
        start_sidecar(FakeFs::default());
        let scope = begin_async_operation(ComponentId::from("fixture.native"), OperationName::from("teardown")).unwrap();
        // The scope is now owned by a thread local, so it is abandoned during
        // this thread's thread-local destruction rather than on a live thread.
        TEARDOWN.with(|slot| *slot.borrow_mut() = Some(AbandonOnTeardown(Some(scope))));
    });
    worker
        .join()
        .expect("abandonment during thread-local teardown must not panic or abort");
}

#[test]
fn nested_capture() {
    start(native_config(["1111111111111103"])).unwrap();
    let mut outer = begin_operation(ComponentId::from("fixture.native"), OperationName::from("outer")).unwrap();
    outer.input("optional", Some(vec![1_i32, 2_i32]).to_native_value()).unwrap();
    emit_event("checkpoint", &BTreeMap::from([("count".to_string(), 2_i32)]));
    let inner = begin_operation(ComponentId::from("fixture.native"), OperationName::from("inner")).unwrap();
    inner.result(Value::Integer(6)).unwrap();
    outer.result(Value::Integer(7)).unwrap();

    let capture = finish().unwrap();
    assert_eq!(capture.operations.len(), 2);
    assert_eq!(capture.operations[0].parent_id, capture.scenario.span_id);
    assert_eq!(capture.operations[1].parent_id, capture.operations[0].span_id);
    assert_ne!(capture.operations[0].span_id, capture.operations[1].span_id);
    assert_eq!(capture.operations[0].observations[0].name, "checkpoint");
    assert!(matches!(&capture.operations[0].inputs["optional"], Value::Map(values) if values.contains_key("Some")));
}

#[test]
fn terminal_capture() {
    start(native_config([])).unwrap();
    let empty = begin_operation(ComponentId::from("fixture.native"), OperationName::from("empty")).unwrap();
    empty.empty().unwrap();
    let error = begin_operation(ComponentId::from("fixture.native"), OperationName::from("error")).unwrap();
    error.error("invalid", Value::String("bad".to_string())).unwrap();
    let panic = std::panic::catch_unwind(|| {
        let _fault = begin_operation(ComponentId::from("fixture.native"), OperationName::from("fault")).unwrap();
        panic!("boom");
    });
    assert!(panic.is_err());

    let capture = finish().unwrap();
    assert!(matches!(capture.operations[0].completion, Some(Completion::Empty { .. })));
    assert!(matches!(capture.operations[1].completion, Some(Completion::Error { .. })));
    assert!(matches!(capture.operations[2].completion, Some(Completion::Fault { .. })));
    assert_eq!(capture.run.status, Status::Error);
}

#[test]
fn identity_projection() {
    for raw in ["", "fixture.valid", " spaces / punctuation ! "] {
        let component = ComponentId::new(raw);
        let operation = OperationName::from(raw);
        let target = ImplementationLabel::from(raw.to_string());

        assert_eq!(component.as_str(), raw);
        assert_eq!(operation.as_str(), raw);
        assert_eq!(target.as_str(), raw);
        assert_eq!(serde_json::to_value(&component).unwrap(), serde_json::json!(raw));
        assert_eq!(serde_json::to_value(&operation).unwrap(), serde_json::json!(raw));
        assert_eq!(serde_json::to_value(&target).unwrap(), serde_json::json!(raw));
        assert_eq!(component.to_native_value(), Value::String(raw.to_string()));
        assert_eq!(operation.to_native_value(), Value::String(raw.to_string()));
        assert_eq!(target.to_native_value(), Value::String(raw.to_string()));
    }
}

#[test]
fn stable_serde() {
    start(native_config([])).unwrap();
    let mut scope = begin_operation(ComponentId::from("fixture.native"), OperationName::from("serde")).unwrap();
    scope.input("large", Value::Unsigned(u64::MAX)).unwrap();
    scope.result(Value::Integer(4)).unwrap();
    let capture = finish().unwrap();
    let json = serde_json::to_string(&capture).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["operations"][0]["component_id"], "fixture.native");
    assert_eq!(value["operations"][0]["operation_name"], "serde");
    assert_eq!(serde_json::from_str::<Capture>(&json).unwrap(), capture);
}

#[test]
fn invalid_config() {
    assert!(TraceId::try_from("bad").unwrap_err().to_string().contains("trace ID"));
    let uppercase_trace = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let uppercase_span = "AAAAAAAAAAAAAAAA";
    assert!(TraceId::try_from(uppercase_trace).unwrap_err().to_string().contains("lowercase"));
    assert!(SpanId::try_from(uppercase_span).unwrap_err().to_string().contains("lowercase"));
    serde_json::from_str::<TraceId>(&format!("\"{uppercase_trace}\"")).unwrap_err();
    serde_json::from_str::<SpanId>(&format!("\"{uppercase_span}\"")).unwrap_err();

    start(native_config(["1111111111111103"])).unwrap();
    assert!(finish().unwrap_err().to_string().contains("consumed 0"));
}

/// Setup-folding declarations cannot be registered from this crate: the
/// workspace forbids `unsafe_code`, and a `#[linkme]` element declares a
/// `link_section` static. These tests drive the folding decision directly with
/// metadata built the same way the annotation macros build it. Discovery
/// rejects every shape below, so none of them describes a shippable component.
mod folding {
    use super::*;
    use crate::registry::{ComponentName, FieldMeta, FieldName, FnName, ModulePath, OpDeps, OpMeta, OpName, RustType};

    const COMPONENT: &str = "fixture.folding";

    fn operation(params: &'static [FieldMeta]) -> OpMeta {
        OpMeta::const_builder(OpDeps {
            name: OpName::new("fold"),
            module_path: ModulePath::new("fixture::folding"),
            fn_name: FnName::new("fold"),
            params,
            return_type: RustType::new("()"),
            component: ComponentName::new(COMPONENT),
        })
        .public(true)
        .build()
    }

    fn producer(fn_name: &'static str, return_type: &'static str, fills: Option<FieldName>) -> OpMeta {
        OpMeta::const_builder(OpDeps {
            name: OpName::new("fold"),
            module_path: ModulePath::new("fixture::folding"),
            fn_name: FnName::new(fn_name),
            params: &[],
            return_type: RustType::new(return_type),
            component: ComponentName::new(COMPONENT),
        })
        .setup(fills)
        .build()
    }

    fn plan(operation: &OpMeta, setups: &[OpMeta]) -> Result<BTreeSet<String>, CaptureError> {
        setup::plan_declared_setups(COMPONENT, "fold", operation, setups.iter()).map(|(_required, filled)| filled)
    }

    fn rejection(operation: &OpMeta, setups: &[OpMeta]) -> String {
        let error = plan(operation, setups).unwrap_err();
        assert_eq!(error.kind(), CaptureErrorKind::SetupAmbiguity);
        error.to_string()
    }

    const SEED: FieldMeta = FieldMeta::new(FieldName::new("seed"), RustType::new("Seed"));
    const LEFT: FieldMeta = FieldMeta::new(FieldName::new("left"), RustType::new("Seed"));
    const RIGHT: FieldMeta = FieldMeta::new(FieldName::new("right"), RustType::new("Seed"));

    #[test]
    fn rejects_unknown_fill_target() {
        let message = rejection(
            &operation(&[SEED]),
            &[producer("make_absent", "Seed", Some(FieldName::new("absent")))],
        );
        assert!(
            message.contains("setup 'fixture::folding::make_absent' fills unknown parameter 'absent'"),
            "{message}"
        );
    }

    #[test]
    fn rejects_ambiguous_return_type() {
        let message = rejection(&operation(&[LEFT, RIGHT]), &[producer("make_seed", "Seed", None)]);
        assert!(
            message.contains("ambiguously matches parameters left, right by its return type 'Seed'"),
            "{message}"
        );
    }

    #[test]
    fn rejects_second_receiver() {
        let message = rejection(
            &operation(&[]),
            &[producer("make_first", "Counter", None), producer("make_second", "Counter", None)],
        );
        assert!(
            message.contains("setup 'fixture::folding::make_second' constructs a second receiver"),
            "{message}"
        );
    }

    #[test]
    fn rejects_duplicate_fill_target() {
        let message = rejection(
            &operation(&[SEED]),
            &[
                producer("fill_first", "Seed", Some(FieldName::new("seed"))),
                producer("fill_second", "Seed", Some(FieldName::new("seed"))),
            ],
        );
        assert!(
            message.contains("setup 'fixture::folding::fill_second' fills parameter 'seed', which an earlier setup already fills"),
            "{message}"
        );
    }

    #[test]
    fn folds_a_receiver_beside_a_filled_parameter() {
        let filled = plan(
            &operation(&[SEED]),
            &[producer("make_seed", "Seed", None), producer("make_receiver", "Counter", None)],
        )
        .unwrap();
        assert_eq!(
            filled,
            BTreeSet::from(["seed".to_string()]),
            "an inferred parameter and a receiver are not a collision"
        );
    }

    #[test]
    fn registry_validation_rejects_a_collision_before_any_operation_runs() {
        let declarations = [
            operation(&[SEED]),
            producer("fill_first", "Seed", Some(FieldName::new("seed"))),
            producer("fill_second", "Seed", Some(FieldName::new("seed"))),
        ];
        let error = setup::validate_declared_folding(&declarations).unwrap_err();
        assert_eq!(error.kind(), CaptureErrorKind::SetupAmbiguity);
        let message = error.to_string();
        assert!(message.contains("fixture.folding::fold"), "{message}");
    }

    #[test]
    fn registry_validation_accepts_foldable_declarations() {
        let declarations = [
            operation(&[SEED]),
            producer("make_seed", "Seed", None),
            producer("make_receiver", "Counter", None),
        ];
        setup::validate_declared_folding(&declarations).unwrap();
    }
}
