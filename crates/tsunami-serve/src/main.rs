use anyhow::Result;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};

mod server;

use server::TsunamiServer;

#[derive(Parser)]
#[command(name = "tsunami-serve", about = "Tsunami MCP server for waveform debugging")]
struct Cli {
    /// Waveform file (FST/VCD) to pre-load as the "default" session.
    file: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let server = TsunamiServer::new();

    if let Some(path) = &cli.file {
        eprintln!("Starting tsunami MCP server for: {path}");
        server
            .open_session(path, Some("default".to_string()))
            .map_err(|e| anyhow::anyhow!(e))?;
    } else {
        eprintln!("Starting tsunami MCP server (no file pre-loaded)");
    }

    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
