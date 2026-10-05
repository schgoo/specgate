//! Focused latency evidence for the native instrumentation hot paths.
//!
//! Run with `cargo bench -p specgate-runtime --bench native_capture`. The
//! inactive paths should remain allocation-free; active operation entry,
//! completion, event emission, and lazy projection are tracked separately so
//! profile regressions can be attributed to one boundary.

use specgate_runtime::capture::test_util::start_with_fs;
use specgate_runtime::capture::{Config, ConfigDeps, SetupProvenance, begin_operation, finish, record_setup, start};
use specgate_runtime::registry;
use specgate_runtime::value::Value;
use specgate_runtime::{ComponentId, OperationName, emit_lazy};
use std::hint::black_box;
use std::time::{Duration, Instant};

const ITERATIONS: u32 = 10_000;
const CHECKPOINTS: u32 = 100;
// Review budgets are deliberately above normal reference-host measurements so
// shared CI noise does not create false failures while order-of-magnitude
// instrumentation regressions remain visible.
const INACTIVE_BUDGET: Duration = Duration::from_micros(10);
const ACTIVE_BUDGET: Duration = Duration::from_micros(100);
const CHECKPOINT_BUDGET: Duration = Duration::from_millis(10);

fn measure(mut operation: impl FnMut()) -> Duration {
    let started = Instant::now();
    for _ in 0..ITERATIONS {
        operation();
    }
    started.elapsed()
}

fn main() {
    let inactive_entry = measure(|| {
        black_box(begin_operation(ComponentId::from("bench.component"), OperationName::from("bench"))).unwrap();
    });
    let inactive_event = measure(|| emit_lazy("bench", || Value::Integer(black_box(1))));
    let setup = SetupProvenance {
        component_id: registry::ComponentName::new("bench.component"),
        operation_name: registry::OpName::new("bench"),
        module_path: registry::ModulePath::new("bench"),
        fn_name: registry::FnName::new("setup"),
        fills: None,
    };
    let inactive_setup = measure(|| record_setup(setup, Vec::new).unwrap());

    let config = Config::builder(ConfigDeps {
        scenario_name: "bench".into(),
        trace_id: "00112233445566778899aabbccddeeff".try_into().unwrap(),
        run_span_id: "0011223344556677".try_into().unwrap(),
        scenario_span_id: "8899aabbccddeeff".try_into().unwrap(),
    })
    .build()
    .unwrap();
    start(config).unwrap();
    let active = measure(|| {
        let mut scope = begin_operation(ComponentId::from("bench.component"), OperationName::from("bench")).unwrap();
        scope.input_lazy("value", || Value::Integer(black_box(1))).unwrap();
        emit_lazy("bench", || Value::Integer(black_box(1)));
        scope.result_lazy(|| Value::Integer(black_box(2))).unwrap();
    });
    black_box(finish()).unwrap();

    let checkpoint_config = Config::builder(ConfigDeps {
        scenario_name: "checkpoint".into(),
        trace_id: "11112222333344445555666677778888".try_into().unwrap(),
        run_span_id: "1111222233334444".try_into().unwrap(),
        scenario_span_id: "5555666677778888".try_into().unwrap(),
    })
    .build()
    .unwrap();
    let probe = start_with_fs(checkpoint_config, "checkpoint.json", None).unwrap();
    let checkpoint_started = Instant::now();
    for _ in 0..CHECKPOINTS {
        begin_operation(ComponentId::from("bench.component"), OperationName::from("checkpoint"))
            .unwrap()
            .unit()
            .unwrap();
    }
    let checkpoint_elapsed = checkpoint_started.elapsed();
    black_box(finish()).unwrap();
    black_box(probe.snapshots());

    let inactive_entry = inactive_entry / ITERATIONS;
    let inactive_event = inactive_event / ITERATIONS;
    let inactive_setup = inactive_setup / ITERATIONS;
    let active = active / ITERATIONS;
    let checkpoint_elapsed = checkpoint_elapsed / CHECKPOINTS;
    assert!(inactive_entry < INACTIVE_BUDGET, "inactive entry exceeded {INACTIVE_BUDGET:?}");
    assert!(inactive_event < INACTIVE_BUDGET, "inactive event exceeded {INACTIVE_BUDGET:?}");
    assert!(
        inactive_setup < INACTIVE_BUDGET,
        "inactive setup check exceeded {INACTIVE_BUDGET:?}"
    );
    assert!(active < ACTIVE_BUDGET, "active instrumentation exceeded {ACTIVE_BUDGET:?}");
    assert!(checkpoint_elapsed < CHECKPOINT_BUDGET, "checkpoint exceeded {CHECKPOINT_BUDGET:?}");
    println!("inactive entry: {inactive_entry:?}/iteration");
    println!("inactive event/projection: {inactive_event:?}/iteration");
    println!("inactive setup check: {inactive_setup:?}/iteration");
    println!("active entry/input/event/completion: {active:?}/iteration");
    println!("durable cumulative checkpoint: {checkpoint_elapsed:?}/operation");
}
