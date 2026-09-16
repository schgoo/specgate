//! `specgate` CLI binary entry point.

use std::process::ExitCode;

use specgate_cli::{capture, discover, extract, run, validate};

fn print_usage() {
    eprintln!(
        "usage: specgate <command> [options] <args>\n\
         \n\
         commands:\n  validate <spec-dir> [--strict] [--spec-only] [--assertions-dir <dir>]\n  run <spec.yaml> [--coverage] [--coverage-threshold <pct>] [--verbose] [--json]\n  extract <package-root> -o|--out <spec.yaml> [--component <name>] [--cases]\n  discover <binding.yaml> --component <id> --registry-id <id> --registry-version <version> -o|--out <registry.ctsc.json> [--target <name>]\n  capture <binding.yaml> --out <dir> [--target <name>] [--component <id>]"
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
        "extract" => cmd_extract(rest),
        "discover" => cmd_discover(rest),
        "capture" => cmd_capture(rest),
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

#[derive(Debug, PartialEq, Eq)]
struct CaptureArgs {
    binding: String,
    target: String,
    component: String,
    out: String,
}

fn parse_capture_args(args: &[String]) -> Result<CaptureArgs, String> {
    let mut binding = None;
    let mut target = String::new();
    let mut component = String::new();
    let mut out = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            flag @ ("--target" | "--component" | "--out") => {
                if i + 1 >= args.len() {
                    return Err(format!("{flag} needs an argument"));
                }
                let value = args[i + 1].clone();
                match flag {
                    "--target" => target = value,
                    "--component" => component = value,
                    "--out" => out = Some(value),
                    _ => {}
                }
                i += 2;
            }
            value if !value.starts_with('-') && binding.is_none() => {
                binding = Some(value.to_string());
                i += 1;
            }
            value => return Err(format!("unexpected argument '{value}'")),
        }
    }
    let binding = binding.ok_or_else(|| "capture requires a binding file argument".to_string())?;
    let out = out.ok_or_else(|| "capture requires --out <dir>".to_string())?;
    Ok(CaptureArgs {
        binding,
        target,
        component,
        out,
    })
}

fn cmd_capture(args: &[String]) -> ExitCode {
    let parsed = match parse_capture_args(args) {
        Ok(parsed) => parsed,
        Err(reason) => {
            eprintln!("error: {reason}");
            return ExitCode::from(2);
        }
    };
    let outcome = capture(&parsed.binding, &parsed.target, &parsed.component, &parsed.out);
    print!("{}", capture::format_outcome(&outcome));
    match outcome {
        capture::CaptureOutcome::Complete { .. } => ExitCode::from(0),
        capture::CaptureOutcome::Error { .. } => ExitCode::from(1),
    }
}

fn cmd_discover(args: &[String]) -> ExitCode {
    let mut binding: Option<String> = None;
    let mut target = String::new();
    let mut component: Option<String> = None;
    let mut registry_id: Option<String> = None;
    let mut registry_version: Option<String> = None;
    let mut out: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            flag @ ("--target" | "--component" | "--registry-id" | "--registry-version" | "-o" | "--out") => {
                if i + 1 >= args.len() {
                    eprintln!("error: {flag} needs an argument");
                    return ExitCode::from(2);
                }

                let value = args[i + 1].clone();
                match flag {
                    "--target" => target = value,
                    "--component" => component = Some(value),
                    "--registry-id" => registry_id = Some(value),
                    "--registry-version" => registry_version = Some(value),
                    "-o" | "--out" => out = Some(value),
                    _ => {}
                }
                i += 2;
            }
            a if !a.starts_with('-') && binding.is_none() => {
                binding = Some(a.to_string());
                i += 1;
            }
            a => {
                eprintln!("error: unexpected argument '{a}'");
                return ExitCode::from(2);
            }
        }
    }

    let Some(binding) = binding else {
        eprintln!("error: discover requires a binding file argument");
        return ExitCode::from(2);
    };
    let Some(component) = component else {
        eprintln!("error: discover requires --component <id>");
        return ExitCode::from(2);
    };
    let Some(registry_id) = registry_id else {
        eprintln!("error: discover requires --registry-id <id>");
        return ExitCode::from(2);
    };
    let Some(registry_version) = registry_version else {
        eprintln!("error: discover requires --registry-version <version>");
        return ExitCode::from(2);
    };
    let Some(out) = out else {
        eprintln!("error: discover requires -o/--out <registry.ctsc.json>");
        return ExitCode::from(2);
    };

    let outcome = discover(&binding, &target, &component, &registry_id, &registry_version, &out);
    print!("{}", discover::format_outcome(&outcome));
    match outcome {
        discover::DiscoverOutcome::Complete { .. } => ExitCode::from(0),
        discover::DiscoverOutcome::Error { .. } => ExitCode::from(1),
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
    let mut verbose = false;
    let mut json = false;
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
            "--verbose" => {
                verbose = true;
                i += 1;
            }
            "--json" => {
                json = true;
                i += 1;
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

    let outcome = run(&spec, verbose, json);
    if json {
        print!("{}", run::format_json(&outcome));
    } else {
        print!("{}", run::format_outcome(&outcome, verbose));
    }
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

fn cmd_extract(args: &[String]) -> ExitCode {
    let mut package_root: Option<String> = None;
    let mut out: Option<String> = None;
    let mut component: Option<String> = None;
    let mut cases = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                if i + 1 >= args.len() {
                    eprintln!("error: {} needs an argument", args[i]);
                    return ExitCode::from(2);
                }
                out = Some(args[i + 1].clone());
                i += 2;
            }
            "--component" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --component needs an argument");
                    return ExitCode::from(2);
                }
                component = Some(args[i + 1].clone());
                i += 2;
            }
            "--cases" => {
                cases = true;
                i += 1;
            }
            a if !a.starts_with('-') && package_root.is_none() => {
                package_root = Some(a.to_string());
                i += 1;
            }
            a => {
                eprintln!("error: unexpected argument '{a}'");
                return ExitCode::from(2);
            }
        }
    }
    let Some(package_root) = package_root else {
        eprintln!("error: extract requires a package_root argument");
        return ExitCode::from(2);
    };
    let Some(out) = out else {
        eprintln!("error: extract requires -o/--out <spec.yaml>");
        return ExitCode::from(2);
    };

    let outcome = extract(&package_root, &out, &component.unwrap_or_default(), cases);
    print!("{}", extract::format_outcome(&outcome));
    match &outcome {
        extract::ExtractOutcome::Error { .. } => ExitCode::from(1),
        extract::ExtractOutcome::Complete { .. } => ExitCode::from(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn parses_capture_command_arguments() {
        assert_eq!(
            parse_capture_args(&args(&[
                "binding.yaml",
                "--out",
                "capture",
                "--target",
                "reference",
                "--component",
                "fixture.add",
            ]))
            .unwrap(),
            CaptureArgs {
                binding: "binding.yaml".to_string(),
                target: "reference".to_string(),
                component: "fixture.add".to_string(),
                out: "capture".to_string(),
            }
        );
    }

    #[test]
    fn capture_parser_requires_output_directory() {
        assert_eq!(
            parse_capture_args(&args(&["binding.yaml"])).unwrap_err(),
            "capture requires --out <dir>"
        );
    }
}
