use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[cfg(test)]
mod tests;

/// Build static weather application artifacts from AEMET `OpenData`.
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
    /// Build a deployable application or data artifact.
    Build(BuildArgs),
}

/// Arguments for selecting the artifact to build.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Artifact to build.
    #[command(subcommand)]
    pub target: BuildTarget,
}

/// Artifacts that can be built.
#[derive(Debug, Subcommand)]
pub enum BuildTarget {
    /// Build the static application shell.
    App(BuildAppArgs),
    /// Build a weather-data snapshot.
    Data(BuildDataArgs),
}

/// Arguments for building the application shell.
#[derive(Debug, Args)]
pub struct BuildAppArgs {
    /// Browser-facing base URL containing the weather data.
    #[arg(long, value_name = "URL")]
    pub data_url: String,

    /// Dedicated directory that will contain the application shell.
    #[arg(long, value_name = "PATH")]
    pub output_dir: PathBuf,
}

/// Arguments for building the weather-data snapshot.
#[derive(Debug, Args)]
pub struct BuildDataArgs {
    /// Dedicated directory that will contain the weather data.
    #[arg(long, value_name = "PATH")]
    pub output_dir: PathBuf,
}
