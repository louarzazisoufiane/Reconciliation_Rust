//! The `recon` binary — oneshot reconciliation from a run config + run id.
//!
//! Exit codes (decision 12): `0` on ANY data outcome (however many diffs), and
//! non-zero ONLY on a genuine failure — `2` config, `3` io/read, `4` engine.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use recon_core::config::RunConfig;
use recon_core::error::ReconError;
use recon_report::regenerate_index;
use recon_schema::FsSchemaStore;
use recon_orch::{generate_run_id, run_oneshot};

/// Fixed-width reconciliation — oneshot runner.
#[derive(Debug, Parser)]
#[command(name = "recon", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run one comparison end-to-end (compare → report → index).
    Run {
        /// Path to the run config YAML.
        #[arg(short, long)]
        config: PathBuf,
        /// Explicit run id (auto-generated if omitted).
        #[arg(long)]
        run_id: Option<String>,
        /// Schema library root.
        #[arg(long, default_value = "schemas")]
        schemas: PathBuf,
    },
    /// Regenerate index.html from an existing reports/manifest.json.
    Index {
        /// Reports directory holding manifest.json.
        #[arg(long, default_value = "reports")]
        reports: PathBuf,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("recon: {e}");
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

fn run(cli: Cli) -> Result<(), ReconError> {
    match cli.command {
        Command::Run {
            config,
            run_id,
            schemas,
        } => {
            let text = std::fs::read_to_string(&config)
                .map_err(|e| ReconError::Io(format!("reading {}: {e}", config.display())))?;
            let cfg = RunConfig::from_yaml(&text)?;
            let store = FsSchemaStore::new(schemas);
            let run_id = run_id.unwrap_or_else(generate_run_id);

            let result = run_oneshot(&cfg, &store, &run_id)?;
            let s = &result.outcome.summary;
            println!(
                "run {} [{}] — matched {} / changed {} / only_a {} / only_b {} / dup_a {} / dup_b {} — {}",
                s.run_id,
                if s.pass { "PASS" } else { "DIFFERENCES" },
                s.matched,
                s.changed,
                s.only_in_a,
                s.only_in_b,
                s.dup_keys_a,
                s.dup_keys_b,
                result.paths.report_html.display(),
            );
            Ok(())
        }
        Command::Index { reports } => {
            regenerate_index(&reports)?;
            println!("regenerated {}", reports.join("index.html").display());
            Ok(())
        }
    }
}
