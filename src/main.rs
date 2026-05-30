//! eth-mempool-watcher — Phase A (smoke test, full bodies).
//!
//! Se connecte au mempool Ethereum L1 via WebSocket et s'abonne a
//! `newPendingTransactions` avec `fullTransactions=true` (extension Geth/Reth
//! supportee par publicnode.com).
//!
//! Avantage par rapport au simple `subscribe_pending_transactions` (hashes
//! seuls) : on a directement le body complet (`from`, `to`, `value`, `input`,
//! `gas_price`...) — zero round-trip RPC supplementaire necessaire pour le
//! decodage en Phase B.
//!
//! Endpoint par defaut : `wss://ethereum-rpc.publicnode.com` (confirme en live,
//! sans cle API, ~10 tx/s). Override : `ETH_WS_URL` dans `.env`.

use alloy::consensus::Transaction;
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

    info!("Subscribe `newPendingTransactions` (full bodies)...");
    let sub = provider.subscribe_full_pending_transactions().await?;
    let mut stream = sub.into_stream();
    info!("Subscription active — Ctrl+C pour arreter");

    let mut count: u64 = 0;
    let mut total: u64 = 0;
    let mut with_to: u64 = 0; // tx qui ont un `to` (= pas un contract deployment)
    let mut total_input_bytes: u64 = 0; // taille moyenne du calldata
    let mut last_log = Instant::now();

    while let Some(tx) = stream.next().await {
        count += 1;
        total += 1;
        let input = tx.inner.input();
        total_input_bytes += input.len() as u64;
        if tx.inner.to().is_some() {
            with_to += 1;
        }

        let elapsed = last_log.elapsed();
        if elapsed >= Duration::from_secs(2) {
            let rate = count as f64 / elapsed.as_secs_f64();
            info!(
                total,
                window = count,
                tx_per_sec = format!("{:.1}", rate),
                pct_with_to = format!(
                    "{:.0}%",
                    100.0 * with_to as f64 / total.max(1) as f64
                ),
                avg_input_bytes = total_input_bytes / total.max(1),
                sample_from = %tx.inner.signer(),
                sample_hash = %tx.inner.hash(),
                "mempool tick"
            );
            count = 0;
            last_log = Instant::now();
        }
    }

    warn!(total, "stream pending tx ferme");
    Ok(())
}
