//! Validation one-shot des hashes detectes ce matin (2026-05-30).
//!
//! Demontre que la logique Phase H.2 (`validate_hashes`) fonctionne sur de la
//! vraie data : on passe les tx hashes que notre detector avait flaguees comme
//! patterns MEV, et on recupere leur verdict on-chain.
//!
//! Run : `cargo run --example validate_known`

use alloy::providers::{ProviderBuilder, WsConnect};
use eth_mempool_watcher::validate::{ValidationOutcome, validate_hash};
use eyre::Result;
use std::collections::HashMap;
use tracing::info;
use tracing_subscriber::EnvFilter;

const ETH_WS: &str = "wss://ethereum-rpc.publicnode.com";

/// Hashes reels detectes par le binaire ce matin (extraits des logs de patterns).
/// (hash, contexte du pattern)
const KNOWN_HASHES: &[(&str, &str)] = &[
    (
        "0x594dedc7f8c0f4d2dcc10e2bb4e89cb9fc21fcc8b79ed8c851fb1afb6af86823",
        "LargeWethSwap 0.6099 ETH (0xb1b2d032AA -> 0x6DEA81C8)",
    ),
    (
        "0xf8f7895c45b25841645114aef157cbe9a0d127da2c96ff8c70c36e37d8b5ddc2",
        "SniperCluster 09:34 sur 0x42bBFa2e",
    ),
    (
        "0x9f8722fc141bb9ada0ce479812e687f7ee8edb668d53c3eaa23565c5a3378de9",
        "SniperCluster 09:34 sur 0x42bBFa2e",
    ),
    (
        "0x94e7c12269b76ebb288881d49575db24be14cab9af32d3405f125e7c75451c8b",
        "SniperCluster 09:34 sur 0x42bBFa2e",
    ),
    (
        "0x8ab5f6b8286d0fc0d4abbeb49c28aa610f494817529892553de827359c062a99",
        "BotRepetition 09:35 (0x8ca0A5d1)",
    ),
    (
        "0x764ef4461fc162579de3c222e9f5f25d16c393d12899542ee45508520c37118c",
        "BotRepetition 09:35 (0x8ca0A5d1)",
    ),
    (
        "0x21eb3f50d3352a1716a9f174756d96aa66c65609a7e25729c540d670ee2fa68c",
        "BotRepetition 10:02 (0xb1b2d032AA)",
    ),
    (
        "0x084ea2b13a2722c555652d8526aa19a579803120f5b8fab02f84bad259dff33c",
        "SniperCluster 10:02 sur 0x6DEA81C8",
    ),
];

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    info!(
        ws = ETH_WS,
        n = KNOWN_HASHES.len(),
        "Validation des hashes connus"
    );
    let provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(ETH_WS))
        .await?;

    let mut counts: HashMap<&'static str, u64> = HashMap::new();

    for (hash_str, context) in KNOWN_HASHES {
        let hash = hash_str.parse()?;
        let outcome = validate_hash(&provider, hash).await?;
        *counts.entry(outcome.label()).or_insert(0) += 1;

        let verdict = match &outcome {
            ValidationOutcome::MinedSuccess { block_number } => {
                format!("MINED_SUCCESS (block {block_number})")
            }
            ValidationOutcome::MinedReverted { block_number } => {
                format!("MINED_REVERTED (block {block_number}) <- bot a perdu, gas brule")
            }
            ValidationOutcome::NotMined => "NOT_MINED (droppee)".to_string(),
        };
        info!(verdict, context, hash = hash_str, "validated");
    }

    info!("=== RECAP ===");
    for (label, n) in &counts {
        info!(verdict = label, count = n, "");
    }

    Ok(())
}
