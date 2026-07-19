use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[cfg(test)]
mod tests;

/// Generate static weather datasets from AEMET `OpenData`.
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
    /// Generate a complete weather-data snapshot.
    Generate(GenerateArgs),
}

/// Arguments for snapshot generation.
#[derive(Debug, Args)]
pub struct GenerateArgs {
    /// Dedicated directory that will contain the generated snapshot.
    #[arg(long, value_name = "PATH")]
    pub output_dir: PathBuf,
}
