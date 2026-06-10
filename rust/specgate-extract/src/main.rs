use std::io::Read;
use std::path::PathBuf;
use std::process;

use clap::Parser;
use specgate_core::emit::emit_specs;
use specgate_core::types::ExtractionResult;
use specgate_core::validate::validate;

#[derive(Parser)]
#[command(name = "specgate-extract", about = "Validate annotations and emit spec YAML")]
struct Cli {
    /// Input file (intermediate JSON). Reads from stdin if omitted.
    #[arg(short, long)]
    input: Option<PathBuf>,

    /// Output directory for .spec.yaml files. Writes to stdout if omitted.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Emit diagnostics as JSON to this file instead of human-readable stderr.
    #[arg(long)]
    diagnostics: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let json_input = match &cli.input {
        Some(path) => std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("error: cannot read {}: {e}", path.display());
            process::exit(2);
        }),
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
                eprintln!("error: cannot read stdin: {e}");
                process::exit(2);
            });
            buf
        }
    };

    let extraction: ExtractionResult = serde_json::from_str(&json_input).unwrap_or_else(|e| {
        eprintln!("error: invalid extraction JSON: {e}");
        process::exit(2);
    });

    let result = validate(&extraction);

    // Output diagnostics
    match &cli.diagnostics {
        Some(path) => {
            let json = serde_json::to_string_pretty(&result.report).unwrap();
            std::fs::write(path, json).unwrap_or_else(|e| {
                eprintln!("error: cannot write diagnostics to {}: {e}", path.display());
                process::exit(2);
            });
        }
        None => {
            eprint!("{}", result.report);
        }
    }

    // Emit spec files
    let specs = emit_specs(&result.operations, &extraction.source_language);

    match &cli.output {
        Some(dir) => {
            std::fs::create_dir_all(dir).unwrap_or_else(|e| {
                eprintln!("error: cannot create output dir {}: {e}", dir.display());
                process::exit(2);
            });
            for spec in &specs {
                let filename = format!("{}.spec.yaml", spec.name);
                let path = dir.join(&filename);
                let yaml = serde_yaml::to_string(spec).unwrap();
                let yaml = format!(
                    "# yaml-language-server: $schema=../../schema/spec-schema.json\n{yaml}"
                );
                std::fs::write(&path, yaml).unwrap_or_else(|e| {
                    eprintln!("error: cannot write {}: {e}", path.display());
                    process::exit(2);
                });
                eprintln!("wrote {}", path.display());
            }
        }
        None => {
            for spec in &specs {
                let yaml = serde_yaml::to_string(spec).unwrap();
                println!("# --- {} ---", spec.name);
                println!(
                    "# yaml-language-server: $schema=../../schema/spec-schema.json"
                );
                print!("{yaml}");
            }
        }
    }

    // Exit code
    if result.report.has_errors() {
        process::exit(2);
    } else if result.report.has_warnings() {
        process::exit(1);
    }
}
