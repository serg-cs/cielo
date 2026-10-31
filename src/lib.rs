use std::env;

use anyhow::{Context, Result};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    aemet::AemetClient,
    cli::{
        BuildArgs, BuildDataArgs, BuildTarget, CatalogArgs, CatalogCommand, Cli, Command,
        DeployArgs, DeployTarget, DeployTargetArgs,
    },
    deployment::DeploymentKind,
};

mod aemet;
mod catalog;
pub mod cli;
mod deployment;
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
    match cli.command {
        Command::Build(args) => build(args).await?,
        Command::Deploy(args) => deploy(args).await?,
        Command::Catalog(args) => catalog_command(args).await?,
    }

    Ok(())
}

async fn build(args: BuildArgs) -> Result<()> {
    match args.target {
        BuildTarget::App(args) => {
            let summary = generation::generate_application(&args.output, &args.data)?;
            info!(
                output = %args.output.display(),
                files = summary.files,
                "app generated"
            );
        }
        BuildTarget::Data(args) => build_data(&args).await?,
    }

    Ok(())
}

async fn build_data(args: &BuildDataArgs) -> Result<()> {
    let client = AemetClient::new(Some(api_key()?), args.dump_sources.clone())?;
    let summary = generation::generate_weather_data(&client, &args.output).await?;
    info!(
        municipalities = summary.municipalities,
        failed = summary.failed,
        files = summary.files,
        bytes = summary.bytes,
        output = %args.output.display(),
        "weather data generated"
    );

    Ok(())
}

async fn catalog_command(args: CatalogArgs) -> Result<()> {
    match args.command {
        CatalogCommand::Update(args) => {
            let client = AemetClient::new(Some(api_key()?), None)?;
            let changes = catalog::update_catalog(&client, &args.directory).await?;
            info!(
                municipalities = changes.municipalities,
                beaches = changes.beaches,
                beaches_added = changes.beaches_added.len(),
                beaches_removed = changes.beaches_removed.len(),
                "catalog updated"
            );
            for (id, name, aemet_name) in &changes.added {
                warn!(
                    id,
                    name, aemet_name, "new municipality: check its Spanish name"
                );
            }
            for (id, name) in &changes.removed {
                warn!(id, name, "municipality no longer listed by AEMET; removed");
            }
            for beach in &changes.beaches_skipped {
                warn!(
                    beach,
                    "beach skipped: its municipality is not in AEMET's list"
                );
            }
        }
    }

    Ok(())
}

/// The AEMET API key, from the environment so it never appears in process
/// arguments or generated files.
fn api_key() -> Result<String> {
    env::var(API_KEY_ENV)
        .ok()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .with_context(|| format!("{API_KEY_ENV} environment variable is not set"))
}

async fn deploy(args: DeployArgs) -> Result<()> {
    match args.target {
        DeployTarget::App(args) => deploy_target(&args, DeploymentKind::App).await?,
        DeployTarget::Data(args) => deploy_target(&args, DeploymentKind::Data).await?,
    }

    Ok(())
}

async fn deploy_target(args: &DeployTargetArgs, kind: DeploymentKind) -> Result<()> {
    // Upload one validated directory without exposing deployment configuration.
    let summary = deployment::deploy_directory(args, kind).await?;
    info!(
        target = kind.as_str(),
        uploaded = summary.uploaded,
        unchanged = summary.skipped,
        deleted = summary.deleted,
        bytes = summary.bytes,
        dry_run = args.dry_run,
        "deployment finished"
    );

    Ok(())
}
