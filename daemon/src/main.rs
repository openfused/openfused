mod server;
mod store;
mod tail;
mod types;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "openfused", about = "HTTP daemon for OpenFused agent messaging")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon: serve local context store over HTTP
    Serve {
        /// Path to the context store
        #[arg(short, long, default_value = ".")]
        store: PathBuf,

        /// Port for the HTTP file server (peers connect to this)
        #[arg(short, long, default_value_t = 2053)]
        port: u16,

        /// Bind address (default: localhost only — use 0.0.0.0 to expose publicly)
        #[arg(short, long, default_value = "127.0.0.1")]
        bind: String,

        /// Public mode: only PROFILE.md + inbox (safe for internet/tunnels).
        /// Without this, full mode serves shared/, knowledge/, CONTEXT.md — LAN/VPN only!
        #[arg(long)]
        public: bool,

        /// Bearer token for A2A route authentication (also reads OPENFUSE_TOKEN env var).
        /// Without this, A2A routes are unauthenticated — do not expose to untrusted networks.
        #[arg(long, env = "OPENFUSE_TOKEN")]
        token: Option<String>,

        /// Max task creation requests per IP per minute (0 to disable). Default: 30.
        #[arg(long, default_value_t = 30)]
        rate_limit: u32,

        /// Auto-delete completed/failed/canceled tasks older than N days (0 to disable). Default: 7.
        #[arg(long, default_value_t = 7)]
        gc_days: u32,

        /// Trust X-Forwarded-For header for rate limiting (only when behind a reverse proxy).
        /// Without this, rate limiting always uses the direct peer IP.
        #[arg(long)]
        trust_proxy: bool,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            store,
            port,
            bind,
            public,
            token,
            rate_limit,
            gc_days,
            trust_proxy,
        } => {
            let store_path = store.canonicalize().unwrap_or(store);
            tracing::info!(
                "Serving context store: {:?} on {}:{}",
                store_path,
                bind,
                port
            );
            server::serve(store_path, &bind, port, public, token, rate_limit, gc_days, trust_proxy).await;
        }
    }
}
