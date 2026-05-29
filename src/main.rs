//! eth-mempool-watcher — Phase A (smoke test).
//!
//! Se connecte au mempool Ethereum L1 via WebSocket et s'abonne a
//! `newPendingTransactions`. Logge un rapport toutes les 2s : nombre de tx
//! recus, debit en tx/s, et le hash de la derniere tx vue.
//!
//! Endpoint par defaut : `wss://ethereum-rpc.publicnode.com` (gratuit, peut
//! ne pas supporter le subscribe pending). Override : `ETH_WS_URL` dans `.env`
//! (ex: Alchemy / QuickNode pour avoir le full tx body en Phase B).

use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use eyre::Result;
use futures_util::StreamExt;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

const ETH_WS_DEFAULT: &str = "wss://ethereum-rpc.publicnode.com";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let url = std::env::var("ETH_WS_URL").unwrap_or_else(|_| ETH_WS_DEFAULT.to_string());
    info!(ws = %url, "Connexion au WS Ethereum mainnet");

    let provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(&url))
        .await?;
    info!("WS connecte");

    info!("Tentative subscribe `newPendingTransactions`...");
    let sub = provider.subscribe_pending_transactions().await?;
    let mut stream = sub.into_stream();
    info!("Subscription active — Ctrl+C pour arreter");

    let mut count: u64 = 0;
    let mut total: u64 = 0;
    let mut last_log = Instant::now();

    while let Some(hash) = stream.next().await {
        count += 1;
        total += 1;

        let elapsed = last_log.elapsed();
        if elapsed >= Duration::from_secs(2) {
            let rate = count as f64 / elapsed.as_secs_f64();
            info!(
                total,
                window_count = count,
                tx_per_sec = format!("{:.1}", rate),
                last_tx = %format!("{hash:#x}"),
                "mempool tick"
            );
            count = 0;
            last_log = Instant::now();
        }
    }

    warn!(total, "stream pending tx ferme");
    Ok(())
}
