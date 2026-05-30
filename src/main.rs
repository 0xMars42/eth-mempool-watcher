//! eth-mempool-watcher — Phase B.
//!
//! Stream les full pending tx bodies via WebSocket (publicnode.com), filtre
//! sur la whitelist de routers DEX (Uniswap V2/V3/Universal Router, 1inch v6),
//! et logge chaque hit avec router name, selector, gas price, et taille
//! du calldata. Stats cumulees toutes les 2s.
//!
//! Phase C (decode des swap parameters via `sol!`) viendra dessus.

use alloy::consensus::Transaction;
use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use eth_mempool_watcher::decode::{DecodedSwap, decode as decode_swap};
use eth_mempool_watcher::detect::{Detection, Detector, Observation, SwapDetails};
use eth_mempool_watcher::routers::{Router, lookup};
use eth_mempool_watcher::track::{PendingTracker, TrackedKind};
use eth_mempool_watcher::validate::{ValidationOutcome, validate_hashes};
use eyre::Result;
use futures_util::StreamExt;
use std::collections::HashMap;
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

    let mut total: u64 = 0;
    let mut router_hits: u64 = 0;
    let mut per_router: HashMap<Router, u64> = HashMap::new();
    let mut last_log = Instant::now();
    let mut detector = Detector::new();
    let mut detections_counts: HashMap<&'static str, u64> = HashMap::new();
    // Phase H.1 : tracker des patterns en attente de validation post-block.
    let mut tracker = PendingTracker::new();
    let mut last_drain = Instant::now();
    // Phase H.2 : compteurs cumules par (kind × outcome).
    let mut validated_counts: HashMap<(TrackedKind, &'static str), u64> = HashMap::new();

    while let Some(tx) = stream.next().await {
        total += 1;

        // Filtre : on ne s'interesse qu'aux tx ciblant un router DEX whitelist.
        let Some(to) = tx.inner.to() else { continue };
        let Some(router) = lookup(to) else { continue };

        router_hits += 1;
        *per_router.entry(router).or_insert(0) += 1;

        let input = tx.inner.input();
        let decoded = decode_swap(router, input);
        let max_fee_gwei = tx.inner.max_fee_per_gas() as f64 / 1e9;

        // Log enrichi : on log les champs riches quand decode reussit, sinon
        // on garde au moins le selector + meta.
        match &decoded {
            DecodedSwap::ExactInput {
                token_in,
                token_out,
                amount_in,
                amount_out_min,
                fee_pips,
                ..
            } => info!(
                router = router.name(),
                label = decoded.short_label(),
                token_in = %token_in,
                token_out = %token_out,
                amount_in = %amount_in,
                amount_out_min = %amount_out_min,
                fee_pips = ?fee_pips,
                from = %tx.inner.signer(),
                max_fee_gwei = format!("{max_fee_gwei:.2}"),
                hash = %tx.inner.hash(),
                "DEX swap decoded"
            ),
            DecodedSwap::ExactInputPath {
                path,
                amount_in,
                amount_out_min,
                ..
            } => {
                let amount_in_effective = if *amount_in == alloy::primitives::U256::ZERO {
                    // V2 swapExactETHForTokens : amountIn = tx.value
                    tx.inner.value()
                } else {
                    *amount_in
                };
                info!(
                    router = router.name(),
                    label = decoded.short_label(),
                    path_len = path.len(),
                    token_in = %path.first().copied().unwrap_or_default(),
                    token_out = %path.last().copied().unwrap_or_default(),
                    amount_in = %amount_in_effective,
                    amount_out_min = %amount_out_min,
                    from = %tx.inner.signer(),
                    max_fee_gwei = format!("{max_fee_gwei:.2}"),
                    hash = %tx.inner.hash(),
                    "DEX swap decoded"
                );
            }
            DecodedSwap::UniversalRouterEnvelope {
                n_commands,
                n_inputs,
                ..
            } => info!(
                router = router.name(),
                label = decoded.short_label(),
                n_commands,
                n_inputs,
                from = %tx.inner.signer(),
                max_fee_gwei = format!("{max_fee_gwei:.2}"),
                input_bytes = input.len(),
                hash = %tx.inner.hash(),
                "UR envelope"
            ),
            DecodedSwap::Multicall { n_inner_calls, .. } => info!(
                router = router.name(),
                label = decoded.short_label(),
                n_inner_calls,
                from = %tx.inner.signer(),
                max_fee_gwei = format!("{max_fee_gwei:.2}"),
                input_bytes = input.len(),
                hash = %tx.inner.hash(),
                "multicall envelope"
            ),
            DecodedSwap::Unknown { .. } => info!(
                router = router.name(),
                label = decoded.short_label(),
                from = %tx.inner.signer(),
                max_fee_gwei = format!("{max_fee_gwei:.2}"),
                input_bytes = input.len(),
                hash = %tx.inner.hash(),
                "unknown selector on whitelisted router"
            ),
        }

        // Alimente le detecteur (extrait Observation depuis le DecodedSwap).
        if let Some(observation) = observation_from_decoded(
            &decoded,
            tx.inner.signer(),
            tx.inner.value(),
            *tx.inner.hash(),
        ) {
            for det in detector.observe(observation) {
                log_detection(&det);
                let key = match det {
                    Detection::SniperCluster { .. } => "sniper_cluster",
                    Detection::BotRepetition { .. } => "bot_repetition",
                    Detection::LargeWethSwap { .. } => "large_weth_swap",
                };
                *detections_counts.entry(key).or_insert(0) += 1;

                // Phase H.1 : enregistrer pour validation post-block.
                let kind = match &det {
                    Detection::SniperCluster { .. } => TrackedKind::SniperCluster,
                    Detection::BotRepetition { .. } => TrackedKind::BotRepetition,
                    Detection::LargeWethSwap { .. } => TrackedKind::LargeWethSwap,
                };
                let hashes = match &det {
                    Detection::SniperCluster { sample_hashes, .. } => sample_hashes.clone(),
                    Detection::BotRepetition { sample_hashes, .. } => sample_hashes.clone(),
                    Detection::LargeWethSwap { hash, .. } => vec![*hash],
                };
                // 1 RPC call par detection (rare) — coût négligeable.
                let current_block = provider.get_block_number().await.unwrap_or(0);
                tracker.track(kind, hashes, current_block);
            }
        }

        // Stats periodiques.
        if last_log.elapsed() >= Duration::from_secs(2) {
            let hit_rate = 100.0 * router_hits as f64 / total.max(1) as f64;
            let per_router_summary: String = per_router
                .iter()
                .map(|(r, n)| format!("{}={}", r.name(), n))
                .collect::<Vec<_>>()
                .join(", ");
            let detections_summary: String = detections_counts
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ");
            let tstats = tracker.stats();
            info!(
                total,
                router_hits,
                hit_rate_pct = format!("{hit_rate:.1}"),
                per_router = per_router_summary,
                detections = if detections_summary.is_empty() {
                    "—".to_string()
                } else {
                    detections_summary
                },
                tracker_pending = tstats.pending,
                tracker_tracked = tstats.total_tracked,
                tracker_ready = tstats.total_ready,
                tracker_expired = tstats.total_expired,
                "stats"
            );
            last_log = Instant::now();
        }

        // Phase H.2 : toutes les 30s, drainer les patterns prets et valider
        // chacun via `eth_getTransactionReceipt`.
        if last_drain.elapsed() >= Duration::from_secs(30) {
            let block = provider.get_block_number().await.unwrap_or(0);
            let ready = tracker.drain_ready_and_purge(block);
            for entry in ready {
                let outcomes = validate_hashes(&provider, &entry.hashes).await;
                for (hash, outcome) in &outcomes {
                    *validated_counts
                        .entry((entry.kind, outcome.label()))
                        .or_insert(0) += 1;
                    match outcome {
                        ValidationOutcome::MinedSuccess { block_number } => info!(
                            kind = ?entry.kind,
                            hash = %hash,
                            block = block_number,
                            verdict = "MINED_SUCCESS",
                            "🔍 VALIDATED"
                        ),
                        ValidationOutcome::MinedReverted { block_number } => info!(
                            kind = ?entry.kind,
                            hash = %hash,
                            block = block_number,
                            verdict = "MINED_REVERTED",
                            note = "bot a paye le gas pour rien, probable race MEV perdue",
                            "🔍 VALIDATED"
                        ),
                        ValidationOutcome::NotMined => info!(
                            kind = ?entry.kind,
                            hash = %hash,
                            verdict = "NOT_MINED",
                            note = "tx droppee du mempool",
                            "🔍 VALIDATED"
                        ),
                    }
                }
            }
            last_drain = Instant::now();
        }
    }

    warn!(total, router_hits, "stream pending tx ferme");
    Ok(())
}

/// Convertit un [`DecodedSwap`] en [`Observation`] alimentable au [`Detector`].
/// Renvoie `None` pour les variantes qui n'ont pas assez d'info (UR envelope,
/// Unknown).
fn observation_from_decoded(
    decoded: &DecodedSwap,
    from: Address,
    tx_value: U256,
    hash: alloy::primitives::B256,
) -> Option<Observation> {
    match decoded {
        DecodedSwap::ExactInput {
            token_in,
            token_out,
            amount_in,
            ..
        } => Some(Observation {
            from,
            hash,
            swap: Some(SwapDetails {
                token_in: *token_in,
                token_out: *token_out,
                amount_in: *amount_in,
            }),
        }),
        DecodedSwap::ExactInputPath {
            path, amount_in, ..
        } => {
            let token_in = *path.first()?;
            let token_out = *path.last()?;
            // V2 swapExactETHForTokens : amount_in dans calldata = 0, on prend tx.value.
            let amount_in_effective = if *amount_in == U256::ZERO {
                tx_value
            } else {
                *amount_in
            };
            Some(Observation {
                from,
                hash,
                swap: Some(SwapDetails {
                    token_in,
                    token_out,
                    amount_in: amount_in_effective,
                }),
            })
        }
        // Envelopes : on alimente une obs "from-only" pour permettre
        // BotRepetition. SniperCluster / LargeWethSwap sont skip cote detector.
        DecodedSwap::UniversalRouterEnvelope { .. } | DecodedSwap::Multicall { .. } => {
            Some(Observation {
                from,
                hash,
                swap: None,
            })
        }
        DecodedSwap::Unknown { .. } => None,
    }
}

fn log_detection(det: &Detection) {
    match det {
        Detection::SniperCluster {
            token_out,
            n_swaps,
            sample_hashes,
        } => info!(
            kind = "SniperCluster",
            token_out = %token_out,
            n_swaps,
            sample_hashes = ?sample_hashes,
            "🎯 PATTERN detected"
        ),
        Detection::BotRepetition {
            from,
            n_swaps,
            sample_hashes,
        } => info!(
            kind = "BotRepetition",
            from = %from,
            n_swaps,
            sample_hashes = ?sample_hashes,
            "🎯 PATTERN detected"
        ),
        Detection::LargeWethSwap {
            from,
            token_out,
            amount_in_wei,
            hash,
        } => {
            // U256 -> u128 (sature a u128::MAX si overflow, pas grave pour l'affichage)
            // -> f64. Realiste pour des montants ETH.
            let wei_u128: u128 = (*amount_in_wei).try_into().unwrap_or(u128::MAX);
            let amount_eth = format!("{:.4}", wei_u128 as f64 / 1e18);
            info!(
                kind = "LargeWethSwap",
                from = %from,
                token_out = %token_out,
                amount_eth,
                hash = %hash,
                "🎯 PATTERN detected"
            );
        }
    }
}
