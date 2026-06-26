//! `specgate` CLI binary entry point.

use std::process::ExitCode;

use specgate_cli::{run, validate};

fn print_usage() {
    eprintln!(
        "usage: specgate <command> [options] <args>\n\
         \n\
         commands:\n  validate <spec-dir> [--strict] [--spec-only] [--assertions-dir <dir>]\n  run <spec.yaml> [--coverage] [--coverage-threshold <pct>]"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }
    let cmd = args[0].clone();
    let rest = &args[1..];
    match cmd.as_str() {
        "validate" => cmd_validate(rest),
        "run" => cmd_run(rest),
        "-h" | "--help" => {
            print_usage();
            ExitCode::from(0)
        }
        _ => {
            eprintln!("error: unknown command '{cmd}'");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn cmd_validate(args: &[String]) -> ExitCode {
    let mut spec_dir: Option<String> = None;
    let mut strict = false;
    let mut assertions_dir = String::new();
    let mut spec_only = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--strict" => {
                strict = true;
                i += 1;
            }
            "--assertions-dir" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --assertions-dir needs an argument");
                    return ExitCode::from(2);
                }
                assertions_dir.clone_from(&args[i + 1]);
                i += 2;
            }
            "--spec-only" => {
                spec_only = true;
                i += 1;
            }
            _ if !a.starts_with("--") && spec_dir.is_none() => {
                spec_dir = Some(a.clone());
                i += 1;
            }
            _ => {
                eprintln!("error: unexpected argument '{a}'");
                return ExitCode::from(2);
            }
        }
    }
    let Some(dir) = spec_dir else {
        eprintln!("error: validate requires a spec directory");
        return ExitCode::from(2);
    };
    let outcome = validate(&dir, strict, spec_only, &assertions_dir);
    print!("{}", validate::format_outcome(&outcome));
    match outcome {
        validate::ValidateOutcome::Pass { .. } => ExitCode::from(0),
        validate::ValidateOutcome::Fail { .. } => ExitCode::from(1),
    }
}

fn cmd_run(args: &[String]) -> ExitCode {
    let mut spec: Option<String> = None;
    let mut coverage = false;
    let mut threshold: Option<f64> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--coverage" => {
                coverage = true;
                i += 1;
            }
            "--coverage-threshold" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --coverage-threshold needs a percentage argument");
                    return ExitCode::from(2);
                }
                let Ok(pct) = args[i + 1].parse::<f64>() else {
                    eprintln!("error: --coverage-threshold must be a number");
                    return ExitCode::from(2);
                };
                threshold = Some(pct);
                coverage = true; // a threshold implies coverage
                i += 2;
            }
            a if !a.starts_with("--") && spec.is_none() => {
                spec = Some(a.to_string());
                i += 1;
            }
            a => {
                eprintln!("error: unexpected argument '{a}'");
                return ExitCode::from(2);
            }
        }
    }
    let Some(spec) = spec else {
        eprintln!("error: run requires a spec file argument");
        return ExitCode::from(2);
    };

    if coverage {
        let outcome = run::run_with_coverage(&spec);
        print!("{}", run::format_coverage(&outcome));
        return ExitCode::from(run::coverage_exit_code(&outcome, threshold));
    }

    let outcome = run(&spec);
    print!("{}", run::format_outcome(&outcome));
    match &outcome {
        run::RunOutcome::Error { .. } => ExitCode::from(1),
        run::RunOutcome::Complete { report } => {
            if report.failed > 0 {
                ExitCode::from(1)
            } else {
                ExitCode::from(0)
            }
        }
    }
}
