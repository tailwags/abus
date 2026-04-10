// SPDX-License-Identifier: EUPL-1.2
use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("abusd starting");

    tokio::signal::ctrl_c().await?;

    info!("shutting down");

    Ok(())
}
