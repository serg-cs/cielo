use anyhow::Result;
use cielo::cli::Cli;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    cielo::init_tracing();
    cielo::run(Cli::parse()).await
}
