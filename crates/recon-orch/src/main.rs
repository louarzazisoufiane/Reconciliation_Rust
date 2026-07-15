//! The `recon-orch` daemon binary: watch configured pairs and reconcile each
//! once both sources are complete.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use recon_orch::{Daemon, DaemonConfig};
use tracing::error;

/// Long-running reconciliation watcher (daemon mode).
#[derive(Debug, Parser)]
#[command(name = "recon-orch", version, about)]
struct Cli {
    /// Path to the daemon config (schemas_dir, poll_seconds, pairs).
    #[arg(short, long)]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(error = %e, "daemon failed");
            ExitCode::from(1)
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let config = DaemonConfig::load(&cli.config)?;
    let daemon = Daemon::new(config);
    daemon.run().await?;
    Ok(())
}
