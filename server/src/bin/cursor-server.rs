//! Starts the Nexusor server executable.
use cursor_server::{App, Config, Result};
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cursor_server=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut config = Config::desktop()?;
    if let Ok(env_config) = Config::from_env() {
        if std::env::var("CURSOR_LISTEN_ADDR").is_ok() {
            config.listen_addr = env_config.listen_addr;
            config.use_persisted_ports = false;
        }
    }

    App::new(config).await?.serve().await
}
