use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[cfg(test)]
mod tests;

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

    /// S3-compatible endpoint override.
    #[arg(long, value_name = "URL")]
    pub endpoint: Option<String>,

    /// AWS signing region override.
    #[arg(long, value_name = "REGION")]
    pub region: Option<String>,

    /// Address buckets as endpoint paths instead of subdomains.
    #[arg(long)]
    pub path_style: bool,
}
