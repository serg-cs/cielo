use std::env;

use anyhow::{Context, Result, bail};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{
    aemet::AemetClient,
    cli::{Cli, Command},
    generate::OutputKind,
};

mod aemet;
pub mod cli;
mod generate;

const API_KEY_ENV: &str = "AEMET_API_KEY";

/// Configure structured logs, honoring `RUST_LOG` when it is present.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("cielo=info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

/// Run the command selected by the user.
///
/// # Errors
///
/// Returns an error when configuration, collection, validation, or publishing
/// fails.
pub async fn run(cli: Cli) -> Result<()> {
    let (args, output_kind) = match cli.command {
        Command::Generate(args) => (args, OutputKind::Site),
        Command::GenerateData(args) => (args, OutputKind::Data),
    };

    // Keep credentials out of process arguments and generated files.
    let api_key = env::var(API_KEY_ENV)
        .with_context(|| format!("{API_KEY_ENV} environment variable is not set"))?;
    if api_key.trim().is_empty() {
        bail!("{API_KEY_ENV} environment variable is empty");
    }

    // Collect and publish the requested output as one complete snapshot.
    let client = AemetClient::new(api_key)?;
    let summary = generate::generate(&client, &args.output_dir, output_kind).await?;
    info!(
        municipalities = summary.municipalities,
        temperature_files = summary.temperature_files,
        output = output_kind.as_str(),
        output_dir = %args.output_dir.display(),
        "weather output generated"
    );

    Ok(())
}
