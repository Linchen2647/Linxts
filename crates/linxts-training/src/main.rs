use clap::{Parser, Subcommand};
use linxts_training::{
    DEFAULT_DATASET_SEED, benchmark_no_ai, benchmark_predictions, generate_dataset,
    validate_dataset,
};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "linxts-training",
    version,
    about = "Linxts-Mini deterministic dataset and benchmark tooling"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Generate {
        #[arg(long, default_value = ".")]
        repository_root: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = DEFAULT_DATASET_SEED)]
        seed: u64,
    },
    Validate {
        #[arg(long)]
        dataset: PathBuf,
    },
    Benchmark {
        #[arg(long)]
        dataset: PathBuf,
        #[arg(long)]
        predictions: Option<PathBuf>,
        #[arg(long, default_value = "linxts-no-ai")]
        system: String,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("linxts-training: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Generate {
            repository_root,
            output,
            seed,
        } => {
            let manifest = generate_dataset(&repository_root, &output, seed)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        Command::Validate { dataset } => {
            let report = validate_dataset(&dataset)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.passed() {
                return Err("dataset validation failed".into());
            }
        }
        Command::Benchmark {
            dataset,
            predictions,
            system,
        } => {
            let report = if let Some(predictions) = predictions {
                benchmark_predictions(&dataset, &predictions, &system)?
            } else {
                benchmark_no_ai(&dataset)?
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
    }
    Ok(())
}
