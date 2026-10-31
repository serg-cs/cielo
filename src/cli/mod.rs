use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[cfg(test)]
mod tests;

pub(crate) const DEFAULT_DEPLOY_CONCURRENCY: u16 = 16;

/// Build and deploy a static weather app and its AEMET data.
#[derive(Debug, Parser)]
#[command(name = "cielo", version, about)]
pub struct Cli {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: Command,
}

/// Supported CLI operations.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build the app or its weather data.
    Build(BuildArgs),
    /// Deploy the app or its weather data.
    Deploy(DeployArgs),
    /// Maintain the catalog of places.
    Catalog(CatalogArgs),
}

/// Catalog operations.
#[derive(Debug, Args)]
pub struct CatalogArgs {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: CatalogCommand,
}

/// Supported catalog operations.
#[derive(Debug, Subcommand)]
pub enum CatalogCommand {
    /// Refresh the catalog files from AEMET (needs `AEMET_API_KEY`). Curated
    /// municipality names are kept; new municipalities are listed for review.
    Update(CatalogUpdateArgs),
}

/// Options for `catalog update`.
#[derive(Debug, Args)]
pub struct CatalogUpdateArgs {
    /// Directory with the catalog files.
    #[arg(long, value_name = "PATH", default_value = "catalog")]
    pub directory: PathBuf,
}

/// Select what to build.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Output to build.
    #[command(subcommand)]
    pub target: BuildTarget,
}

/// Supported build outputs.
#[derive(Debug, Subcommand)]
pub enum BuildTarget {
    /// Build the static app.
    App(BuildAppArgs),
    /// Build the weather data.
    Data(BuildDataArgs),
}

/// Build options for the app.
#[derive(Debug, Args)]
pub struct BuildAppArgs {
    /// Directory that will contain the generated app.
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,

    /// Browser-facing URL containing the weather data.
    #[arg(short, long, value_name = "URL")]
    pub data: String,
}

/// Build options for the weather data.
#[derive(Debug, Args)]
pub struct BuildDataArgs {
    /// Directory that will contain the generated weather data.
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,

    /// Save the raw AEMET payloads in this directory, for test fixtures.
    #[arg(long, value_name = "PATH", hide = true)]
    pub dump_sources: Option<PathBuf>,
}

/// Select what to deploy.
#[derive(Debug, Args)]
pub struct DeployArgs {
    /// Target to deploy.
    #[command(subcommand)]
    pub target: DeployTarget,
}

/// Supported deployment targets.
#[derive(Debug, Subcommand)]
pub enum DeployTarget {
    /// Deploy the static app.
    App(DeployTargetArgs),
    /// Deploy the weather data.
    Data(DeployTargetArgs),
}

/// Shared S3 deployment options.
#[derive(Debug, Args)]
pub struct DeployTargetArgs {
    /// Directory containing the files to deploy.
    #[arg(short, long, value_name = "PATH")]
    pub input: PathBuf,

    /// Destination S3 bucket.
    #[arg(short, long, value_name = "NAME")]
    pub bucket: String,

    /// Maximum number of object uploads in progress.
    #[arg(
        long,
        value_name = "COUNT",
        default_value_t = DEFAULT_DEPLOY_CONCURRENCY,
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub concurrency: u16,

    /// S3-compatible endpoint override.
    #[arg(long, value_name = "URL")]
    pub endpoint: Option<String>,

    /// AWS signing region override.
    #[arg(long, value_name = "REGION")]
    pub region: Option<String>,

    /// Address buckets as endpoint paths instead of subdomains.
    #[arg(long)]
    pub path_style: bool,

    /// Delete remote objects under this target's prefixes that are not in the input.
    #[arg(long)]
    pub prune: bool,

    /// Show what would be uploaded or deleted without changing the bucket.
    #[arg(long)]
    pub dry_run: bool,
}
