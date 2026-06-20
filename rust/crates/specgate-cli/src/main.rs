//! `specgate` CLI binary entry point.

use std::process::ExitCode;

use specgate_cli::{run, validate};

fn print_usage() {
    eprintln!(
        "usage: specgate <command> [options] <args>\n\
         \n\
         commands:\n  validate <spec-dir> [--strict] [--suppress check,check] [--assertions-dir <dir>] [--check-source]\n  run <spec.yaml>"
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
    let mut suppress: Vec<String> = Vec::new();
    let mut assertions_dir = String::new();
    let mut check_source = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--strict" => {
                strict = true;
                i += 1;
            }
            "--suppress" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --suppress needs an argument");
                    return ExitCode::from(2);
                }
                for tok in args[i + 1].split(',') {
                    let t = tok.trim();
                    if !t.is_empty() {
                        suppress.push(t.to_string());
                    }
                }
                i += 2;
            }
            "--assertions-dir" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --assertions-dir needs an argument");
                    return ExitCode::from(2);
                }
                assertions_dir = args[i + 1].clone();
                i += 2;
            }
            "--check-source" => {
                check_source = true;
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
    let outcome = validate::validate(&dir, strict, &suppress, &assertions_dir, check_source);
    print!("{}", validate::format_outcome(&outcome));
    match outcome {
        validate::ValidateOutcome::Pass { .. } => ExitCode::from(0),
        validate::ValidateOutcome::Fail { .. } => ExitCode::from(1),
    }
}

fn cmd_run(args: &[String]) -> ExitCode {
    if args.len() != 1 {
        eprintln!("error: run requires exactly one spec file argument");
        return ExitCode::from(2);
    }
    let outcome = run::run(&args[0]);
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
