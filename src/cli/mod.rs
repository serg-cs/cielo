use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[cfg(test)]
mod tests;

/// Generate a static weather website from AEMET `OpenData`.
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
    /// Generate a complete static website and weather-data snapshot.
    Generate(GenerateArgs),
    /// Generate only a weather-data snapshot.
    GenerateData(GenerateArgs),
}

/// Arguments for generated output.
#[derive(Debug, Args)]
pub struct GenerateArgs {
    /// Dedicated directory that will contain the generated output.
    #[arg(long, value_name = "PATH")]
    pub output_dir: PathBuf,
}
