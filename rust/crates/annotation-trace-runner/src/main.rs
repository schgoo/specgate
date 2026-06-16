use std::env;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [case_name, source_arg, driver_arg] = args.as_slice() else {
        eprintln!("usage: annotation-trace-runner <case_name> <source> <driver>");
        std::process::exit(2);
    };

    let result = annotation_trace_runner::run(case_name, source_arg, driver_arg)
        .expect("annotation trace runner should succeed");
    print!(
        "{}",
        serde_json::to_string(&result).expect("runner output should serialize")
    );
}
