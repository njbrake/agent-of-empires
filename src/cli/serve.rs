//! `aoe serve` command -- start a web dashboard for remote session access

use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// Host/IP to bind to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Disable authentication (WARNING: anyone on the network can control your sessions)
    #[arg(long)]
    pub no_auth: bool,
}

pub async fn run(profile: &str, args: ServeArgs) -> Result<()> {
    crate::server::start_server(profile, &args.host, args.port, args.no_auth).await
}
