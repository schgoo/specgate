use std::env;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let (case_name, source_arg, driver_arg) = match args.as_slice() {
        [source] => (None, source.as_str(), None),
        [case_name, source] => (Some(case_name.as_str()), source.as_str(), None),
        [case_name, source, driver] => (
            Some(case_name.as_str()),
            source.as_str(),
            Some(driver.as_str()),
        ),
        _ => {
            eprintln!("usage: annotation-test-runner [case_name] <source> [driver]");
            std::process::exit(2);
        }
    };

    let cwd = env::current_dir().expect("current directory should resolve");
    let result = annotation_test_runner::run(case_name, source_arg, driver_arg, &cwd)
        .expect("annotation test runner should succeed");
    print!(
        "{}",
        serde_json::to_string(&result).expect("runner output should serialize")
    );
}
