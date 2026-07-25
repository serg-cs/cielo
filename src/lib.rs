use std::env;

use anyhow::{Context, Result, bail};
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{
    aemet::AemetClient,
    cli::{BuildDataArgs, BuildTarget, Cli, Command},
};

mod aemet;
pub mod cli;
mod generation;

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
    let Command::Build(args) = cli.command;
    match args.target {
        BuildTarget::App(args) => {
            let summary = generation::generate_application(&args.output, &args.data)?;
            info!(
                data_url = args.data,
                output = %args.output.display(),
                files = summary.files,
                bytes = summary.bytes,
                "app generated"
            );
        }
        BuildTarget::Data(args) => build_data(&args).await?,
    }

    Ok(())
}

async fn build_data(args: &BuildDataArgs) -> Result<()> {
    // Keep credentials out of process arguments and generated files.
    let api_key = env::var(API_KEY_ENV)
        .with_context(|| format!("{API_KEY_ENV} environment variable is not set"))?;
    if api_key.trim().is_empty() {
        bail!("{API_KEY_ENV} environment variable is empty");
    }

    // Collect and publish one complete data snapshot.
    let client = AemetClient::new(api_key)?;
    let summary = generation::generate_weather_data(&client, &args.output).await?;
    info!(
        municipalities = summary.municipalities,
        forecast_files = summary.forecast_files,
        files = summary.files,
        bytes = summary.bytes,
        output = %args.output.display(),
        "weather data generated"
    );

    Ok(())
}
