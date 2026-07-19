use std::env;

use anyhow::{Context, Result, bail};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{
    aemet::AemetClient,
    cli::{Cli, Command},
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
    match cli.command {
        Command::Generate(args) => {
            // Keep credentials out of process arguments and generated files.
            let api_key = env::var(API_KEY_ENV)
                .with_context(|| format!("{API_KEY_ENV} environment variable is not set"))?;
            if api_key.trim().is_empty() {
                bail!("{API_KEY_ENV} environment variable is empty");
            }

            // Collect and publish a complete snapshot in one operation.
            let client = AemetClient::new(api_key)?;
            let summary = generate::generate(&client, &args.output_dir).await?;
            info!(
                municipalities = summary.municipalities,
                temperature_files = summary.temperature_files,
                output_dir = %args.output_dir.display(),
                "weather data generated"
            );
        }
    }

    Ok(())
}
