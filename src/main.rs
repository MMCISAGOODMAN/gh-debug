mod actions;
mod cli;
mod docker;
mod env;
mod session;
mod tui;
mod web;
mod workflow;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.execute().await
}
